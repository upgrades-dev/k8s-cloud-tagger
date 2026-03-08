use crate::cloud::{CloudClient, Labels};
use crate::error::Error;
use crate::tls::http_client;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::collections::BTreeMap;

const ARM_API_VERSION: &str = "2024-03-02";
const IMDS_TOKEN_URL: &str = "http://169.254.169.254/metadata/identity/oauth2/token\
     ?api-version=2018-02-01\
     &resource=https%3A%2F%2Fmanagement.azure.com%2F";

/// A parsed Azure Managed Disk ARM resource ID.
///
/// The CSI volume handle for `disk.csi.azure.com` is the full ARM resource ID:
///   `/subscriptions/<sub>/resourceGroups/<rg>/providers/Microsoft.Compute/disks/<name>`
pub struct AzureDisk {
    pub resource_id: String,
}

impl AzureDisk {
    /// Validate and wrap an ARM resource ID for a managed disk.
    ///
    /// Expected shape (case-insensitive provider segment):
    ///   `/subscriptions/<sub>/resourceGroups/<rg>/providers/Microsoft.Compute/disks/<name>`
    pub fn parse(resource_id: &str) -> Option<Self> {
        // Split on '/' — leading '/' yields an empty first element.
        let parts: Vec<&str> = resource_id.split('/').collect();
        // ["", "subscriptions", sub, "resourceGroups", rg,
        //  "providers", "Microsoft.Compute", "disks", name]
        if parts.len() != 9 {
            return None;
        }
        if parts[0] != ""
            || !parts[1].eq_ignore_ascii_case("subscriptions")
            || !parts[3].eq_ignore_ascii_case("resourceGroups")
            || !parts[5].eq_ignore_ascii_case("providers")
            || !parts[6].eq_ignore_ascii_case("Microsoft.Compute")
            || !parts[7].eq_ignore_ascii_case("disks")
        {
            return None;
        }
        Some(Self {
            resource_id: resource_id.to_string(),
        })
    }

    /// Build the ARM management URL for this disk.
    pub fn api_url(&self) -> String {
        format!(
            "https://management.azure.com{}?api-version={}",
            self.resource_id, ARM_API_VERSION
        )
    }
}

/// Sanitise a string for use as an Azure resource tag key or value.
///
/// Azure tag constraints:
/// - Keys: max 512 chars; must not contain `<`, `>`, `%`, `&`, `\`, `?`, `/`
/// - Values: max 256 chars
///
/// We replace disallowed characters with `-` and truncate to the limit.
fn sanitise_azure_tag_key(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            '<' | '>' | '%' | '&' | '\\' | '?' | '/' => '-',
            _ => c,
        })
        .take(512)
        .collect()
}

fn sanitise_azure_tag_value(input: &str) -> String {
    input.chars().take(256).collect()
}

fn sanitise_tags(labels: &Labels) -> BTreeMap<String, String> {
    labels
        .iter()
        .map(|(k, v)| (sanitise_azure_tag_key(k), sanitise_azure_tag_value(v)))
        .collect()
}

#[derive(Deserialize)]
struct ImdsTokenResponse {
    access_token: String,
}

pub struct AzureClient {
    http: Client,
}

impl AzureClient {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            http: http_client()?,
        })
    }

    /// Make a bearer token using the Azure Instance Metadata Service (IMDS).
    ///
    /// This works on any Azure VM or AKS node/pod with a managed identity assigned.
    async fn imds_token(&self) -> Result<String, Error> {
        let resp: ImdsTokenResponse = self
            .http
            .get(IMDS_TOKEN_URL)
            .header("Metadata", "true")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp.access_token)
    }
}

#[async_trait]
impl CloudClient for AzureClient {
    fn provider_name(&self) -> &'static str {
        "azure"
    }

    async fn set_tags(&self, resource_id: &str, labels: &Labels) -> Result<(), Error> {
        let disk = AzureDisk::parse(resource_id)
            .ok_or_else(|| Error::CloudApi(format!("Invalid Azure resource ID: {resource_id}")))?;

        let token = self.imds_token().await?;
        let tags = sanitise_tags(labels);

        let body = serde_json::json!({ "tags": tags });

        self.http
            .patch(disk.api_url())
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        tracing::debug!(
            disk = %resource_id,
            ?tags,
            "Azure: tags set"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_disk() {
        let id =
            "/subscriptions/sub-id/resourceGroups/my-rg/providers/Microsoft.Compute/disks/my-disk";
        let disk = AzureDisk::parse(id).unwrap();
        assert_eq!(disk.resource_id, id);
        assert_eq!(
            disk.api_url(),
            format!(
                "https://management.azure.com{}?api-version={}",
                id, ARM_API_VERSION
            )
        );
    }

    #[test]
    fn parse_invalid() {
        assert!(AzureDisk::parse("not-a-resource-id").is_none());
        assert!(AzureDisk::parse("").is_none());
        // Wrong provider
        assert!(
            AzureDisk::parse(
                "/subscriptions/s/resourceGroups/rg/providers/Microsoft.Storage/storageAccounts/sa"
            )
            .is_none()
        );
        // Too few segments
        assert!(
            AzureDisk::parse(
                "/subscriptions/s/resourceGroups/rg/providers/Microsoft.Compute/disks"
            )
            .is_none()
        );
    }

    #[test]
    fn sanitise_tag_key_replaces_disallowed() {
        assert_eq!(
            sanitise_azure_tag_key("app.kubernetes.io/name"),
            "app.kubernetes.io-name"
        );
        assert_eq!(
            sanitise_azure_tag_key("key<with>bad%chars"),
            "key-with-bad-chars"
        );
        assert_eq!(sanitise_azure_tag_key("normal-key"), "normal-key");
    }

    #[test]
    fn sanitise_tag_key_truncates() {
        let long = "a".repeat(600);
        assert_eq!(sanitise_azure_tag_key(&long).len(), 512);
    }

    #[test]
    fn sanitise_tag_value_truncates() {
        let long = "v".repeat(300);
        assert_eq!(sanitise_azure_tag_value(&long).len(), 256);
    }

    #[test]
    fn sanitise_labels() {
        let mut labels = BTreeMap::new();
        labels.insert("app.kubernetes.io/name".to_string(), "frontend".to_string());
        labels.insert("env".to_string(), "prod".to_string());
        let result = sanitise_tags(&labels);
        assert_eq!(result["app.kubernetes.io-name"], "frontend");
        assert_eq!(result["env"], "prod");
    }
}
