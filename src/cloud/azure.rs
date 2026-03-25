use crate::cloud::{CloudClient, Labels};
use crate::error::Error;
use crate::tls::http_client;
use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;
use std::collections::BTreeMap;

const TAGS_API_VERSION: &str = "2021-04-01";
const DEFAULT_AUTHORITY_HOST: &str = "https://login.microsoftonline.com/";
const ARM_SCOPE: &str = "https://management.azure.com/.default";
const CLIENT_ASSERTION_TYPE: &str = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";

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
        if !parts[0].is_empty()
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

    /// Build the ARM Tags API URL for this disk.
    pub fn tags_url(&self) -> String {
        format!(
            "https://management.azure.com{}/providers/Microsoft.Resources/tags/default?api-version={}",
            self.resource_id, TAGS_API_VERSION
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

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Serialize)]
struct TagsPatch {
    operation: &'static str,
    properties: TagsProperties,
}

#[derive(Serialize)]
struct TagsProperties {
    tags: BTreeMap<String, String>,
}

pub struct AzureClient {
    http: Client,
    client_id: String,
    tenant_id: String,
    authority_host: String,
    federated_token_file: String,
}

impl AzureClient {
    pub fn new() -> Result<Self, Error> {
        let client_id = std::env::var("AZURE_CLIENT_ID")
            .map_err(|_| Error::Azure("AZURE_CLIENT_ID not set".into()))?;
        let tenant_id = std::env::var("AZURE_TENANT_ID")
            .map_err(|_| Error::Azure("AZURE_TENANT_ID not set".into()))?;
        let authority_host = std::env::var("AZURE_AUTHORITY_HOST")
            .unwrap_or_else(|_| DEFAULT_AUTHORITY_HOST.to_string());
        let federated_token_file = std::env::var("AZURE_FEDERATED_TOKEN_FILE")
            .map_err(|_| Error::Azure("AZURE_FEDERATED_TOKEN_FILE not set".into()))?;
        Ok(Self {
            http: http_client()?,
            client_id,
            tenant_id,
            authority_host,
            federated_token_file,
        })
    }

    /// Obtain a bearer token using AKS Workload Identity.
    ///
    /// The AKS Workload Identity webhook injects four environment variables:
    /// - `AZURE_FEDERATED_TOKEN_FILE` — path to the projected K8s service account token
    /// - `AZURE_CLIENT_ID`            — the managed identity's client ID
    /// - `AZURE_TENANT_ID`            — the Azure AD tenant ID
    /// - `AZURE_AUTHORITY_HOST`       — AAD endpoint (defaults to https://login.microsoftonline.com/)
    ///
    /// The K8s token is exchanged for an ARM bearer token via the OAuth 2.0
    /// client credentials flow with a federated assertion.
    async fn workload_identity_token(&self) -> Result<String, Error> {
        let assertion = std::fs::read_to_string(&self.federated_token_file).map_err(|e| {
            Error::Azure(format!("Failed to read {}: {e}", self.federated_token_file))
        })?;

        let url = format!(
            "{}{}/oauth2/v2.0/token",
            self.authority_host, self.tenant_id
        );

        let resp: TokenResponse = self
            .http
            .post(&url)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_assertion_type", CLIENT_ASSERTION_TYPE),
                ("client_assertion", assertion.trim()),
                ("client_id", &self.client_id),
                ("scope", ARM_SCOPE),
            ])
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

        let token = self.workload_identity_token().await?;

        let sanitised = sanitise_tags(labels);
        let body = TagsPatch {
            operation: "Merge",
            properties: TagsProperties {
                tags: sanitised.clone(),
            },
        };

        self.http
            .patch(disk.tags_url())
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        tracing::debug!(
            disk = %resource_id,
            tags = ?sanitised,
            "Azure: tags merged"
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
            disk.tags_url(),
            format!(
                "https://management.azure.com{}/providers/Microsoft.Resources/tags/default?api-version={}",
                id, TAGS_API_VERSION
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
