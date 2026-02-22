use crate::cloud::{CloudClient, Labels};
use crate::error::Error;
use crate::tls::http_client;
use async_trait::async_trait;
use gcp_auth::TokenProvider;
use reqwest::Client;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct GcpDisk {
    pub project: String,
    pub location: String,
    pub regional: bool,
    pub name: String,
}

impl GcpDisk {
    /// Parse a CSI volume handle into a GcpDisk.
    ///
    /// Zonal:   "projects/my-proj/zones/us-east1-b/disks/pvc-abc"
    /// Regional: "projects/my-proj/regions/us-east1/disks/pvc-abc"
    pub fn parse(volume_handle: &str) -> Option<Self> {
        let parts: Vec<&str> = volume_handle.split('/').collect();
        // Expected: ["projects", proj, "zones"|"regions", loc, "disks", name]
        if parts.len() != 6 || parts[0] != "projects" || parts[4] != "disks" {
            return None;
        }

        let regional = match parts[2] {
            "zones" => false,
            "regions" => true,
            _ => return None,
        };

        Some(Self {
            project: parts[1].to_string(),
            location: parts[3].to_string(),
            regional,
            name: parts[5].to_string(),
        })
    }

    /// Build the Compute API URL path for this disk.
    pub fn api_path(&self) -> String {
        let loc_type = if self.regional { "regions" } else { "zones" };
        format!(
            "https://compute.googleapis.com/compute/v1/projects/{}/{}/{}/disks/{}",
            self.project, loc_type, self.location, self.name
        )
    }
}

/// Sanitise a string for use as a GCP label key or value.
///
/// GCP labels allow `[a-z0-9_-]`, max 63 chars.
/// We follow GCP's own label conventions (e.g. `goog-gke-*`), using
/// hyphens as the standard separator.
fn sanitise_gcp_label(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' | '-' | '_' => c,
            _ => '-',
        })
        .take(63)
        .collect()
}

/// Sanitise a Kubernetes label key for use as a GCP label key.
/// Returns None if the result is empty after sanitisation.
fn sanitise_gcp_label_key(input: &str) -> Option<String> {
    let s = sanitise_gcp_label(input);
    let s = s.trim_start_matches(|c: char| !c.is_ascii_lowercase());
    (!s.is_empty()).then(|| s.to_string())
}

/// Sanitise a full set of Kubernetes labels for GCP.
/// Keys that are empty after sanitisation are skipped.
fn sanitise_labels(labels: &Labels) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for (k, v) in labels {
        match sanitise_gcp_label_key(k) {
            Some(gcp_key) => {
                let gcp_val = sanitise_gcp_label(v);
                tracing::debug!(k8s_key = %k, gcp_key = %gcp_key, "Sanitised label key");
                result.insert(gcp_key, gcp_val);
            }
            None => {
                tracing::debug!(key = %k, "Skipping label: empty after sanitisation");
            }
        }
    }
    result
}

#[derive(Deserialize)]
struct DiskResponse {
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(rename = "labelFingerprint")]
    label_fingerprint: String,
}

pub struct GcpClient {
    http: Client,
    auth: Arc<dyn TokenProvider>,
}

impl GcpClient {
    pub async fn new() -> Result<Self, Error> {
        let provider = gcp_auth::provider().await?;
        Ok(Self {
            http: http_client()?,
            auth: provider,
        })
    }

    async fn token(&self) -> Result<String, Error> {
        // TODO(afharvey) the scope can be smaller, only the compute API for example.
        let scopes = &["https://www.googleapis.com/auth/cloud-platform"];
        let token = self.auth.token(scopes).await?;
        Ok(token.as_str().to_string())
    }

    async fn get_disk_response(&self, disk: &GcpDisk) -> Result<DiskResponse, Error> {
        let token = self.token().await?;
        let resp: DiskResponse = self
            .http
            .get(disk.api_path())
            .bearer_auth(&token)
            .query(&[("fields", "labels,labelFingerprint")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(resp)
    }

    async fn post_labels(
        &self,
        disk: &GcpDisk,
        labels: &BTreeMap<String, String>,
        fingerprint: &str,
    ) -> Result<(), Error> {
        let token = self.token().await?;
        let body = serde_json::json!({
            "labels": labels,
            "labelFingerprint": fingerprint,
        });

        self.http
            .post(format!("{}/setLabels", disk.api_path()))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}

#[async_trait]
impl CloudClient for GcpClient {
    fn provider_name(&self) -> &'static str {
        "gcp"
    }

    async fn set_tags(&self, resource_id: &str, labels: &Labels) -> Result<(), Error> {
        let disk =
            GcpDisk::parse(resource_id).ok_or(Error::CloudApi("Invalid resource ID".into()))?;

        let disk_response = self.get_disk_response(&disk).await?;

        let sanitised = sanitise_labels(labels);
        let mut merged = disk_response.labels;
        merged.extend(sanitised);

        self.post_labels(&disk, &merged, &disk_response.label_fingerprint)
            .await?;

        tracing::debug!(
            disk = %resource_id,
            labels = ?merged,
            "GCP: labels set"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_zonal() {
        let d = GcpDisk::parse("projects/my-proj/zones/us-east1-b/disks/pvc-abc").unwrap();
        assert_eq!(d.project, "my-proj");
        assert_eq!(d.location, "us-east1-b");
        assert!(!d.regional);
        assert_eq!(d.name, "pvc-abc");
    }

    #[test]
    fn parse_regional() {
        let d = GcpDisk::parse("projects/my-proj/regions/us-east1/disks/pvc-abc").unwrap();
        assert_eq!(d.project, "my-proj");
        assert_eq!(d.location, "us-east1");
        assert!(d.regional);
        assert_eq!(d.name, "pvc-abc");
    }

    #[test]
    fn parse_invalid() {
        assert!(GcpDisk::parse("not-a-handle").is_none());
        assert!(GcpDisk::parse("projects/p/something/z/disks/d").is_none());
        assert!(GcpDisk::parse("").is_none());
    }
}
