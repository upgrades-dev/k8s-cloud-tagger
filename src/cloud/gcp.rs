use crate::cloud::{CloudClient, Labels};
use crate::error::Error;
use crate::tls::http_client;
use async_trait::async_trait;
use gcp_auth::TokenProvider;
use reqwest::Client;
use serde::Deserialize;
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

#[derive(Deserialize)]
struct DiskResponse {
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

    async fn get_label_fingerprint(&self, disk: &GcpDisk) -> Result<String, Error> {
        let token = self.token().await?;
        let resp: DiskResponse = self
            .http
            .get(disk.api_path())
            .bearer_auth(&token)
            .query(&[("fields", "labelFingerprint")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(resp.label_fingerprint)
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

        let fingerprint = self.get_label_fingerprint(&disk).await?;
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

        tracing::debug!(
            disk = %resource_id,
            lalbels = ?labels,
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
