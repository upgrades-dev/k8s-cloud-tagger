use crate::cloud::CloudClient;
use crate::error::Error;
use crate::tls::http_client;
use async_trait::async_trait;
use aws_credential_types::Credentials;
use aws_sigv4::http_request::{SignableBody, SignableRequest, SigningSettings, sign};
use aws_sigv4::sign::v4;
use aws_smithy_runtime_api::client::identity::Identity;
use reqwest::Client;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::SystemTime;

/// An AWS EBS volume resource.
///
/// The EBS CSI driver provides the volume ID (e.g., `vol-0123456789cafe0`) in the
/// PersistentVolume's `csi.volumeHandle` field. We construct the full ARN from the
/// volume ID, region, and account ID provided by the AwsClient.
pub struct AwsDisk {
    pub region: String,
    pub volume_id: String,
    #[allow(dead_code)]
    pub arn: String,
}

impl AwsDisk {
    /// Create an AwsDisk from a volume ID.
    ///
    /// The EBS CSI driver returns volume IDs like `vol-0123456789cafe0`.
    /// We construct the full ARN: `arn:aws:ebs:{region}:{account}:volume/{volume-id}`
    pub fn parse(volume_id: &str, region: &str, account_id: &str) -> Option<Self> {
        if volume_id.is_empty() {
            return None;
        }

        Some(Self {
            region: region.to_string(),
            volume_id: volume_id.to_string(),
            arn: format!("arn:aws:ebs:{}:{}:volume/{}", region, account_id, volume_id),
        })
    }

    /// Build the EC2 endpoint URL for this volume's region.
    pub fn endpoint(&self) -> String {
        format!("https://ec2.{}.amazonaws.com/", self.region)
    }
}

pub type Labels = BTreeMap<String, String>;

/// Sanitise a string for use as an AWS resource tag key.
///
/// AWS tag constraints:
/// - Keys: max 128 chars; must not contain `<`, `>`, `%`, `&`, `\`, `?`, `/`
/// - Reserved: `aws:` prefix is reserved by AWS and will be rejected
fn sanitise_aws_tag_key(input: &str) -> Option<String> {
    let sanitized: String = input
        .chars()
        .map(|c| match c {
            '<' | '>' | '%' | '&' | '\\' | '?' | '/' => '-',
            _ => c,
        })
        .take(128)
        .collect();

    if sanitized.starts_with("aws:") {
        tracing::debug!(key = %input, "Skipping AWS tag: reserved prefix");
        None
    } else {
        Some(sanitized)
    }
}

fn sanitise_aws_tag_value(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            '<' | '>' | '%' | '&' | '\\' | '?' | '/' => '-',
            _ => c,
        })
        .take(256)
        .collect()
}

fn sanitise_tags(labels: &Labels) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for (k, v) in labels {
        match sanitise_aws_tag_key(k) {
            Some(aws_key) => {
                let aws_val = sanitise_aws_tag_value(v);
                tracing::debug!(k8s_key = %k, aws_key = %aws_key, "Sanitised AWS tag key");
                result.insert(aws_key, aws_val);
            }
            None => {
                tracing::debug!(key = %k, "Skipping AWS tag: reserved prefix");
            }
        }
    }
    result
}

/// AWS temporary credentials from STS.
#[derive(Debug)]
pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

/// XML response structure for STS AssumeRoleWithWebIdentity.
#[derive(Debug, Deserialize)]
struct StsResponse {
    #[serde(rename = "AssumeRoleWithWebIdentityResult")]
    result: AssumeRoleResult,
}

#[derive(Debug, Deserialize)]
struct AssumeRoleResult {
    #[serde(rename = "Credentials")]
    credentials: CredentialsElement,
}

#[derive(Debug, Deserialize)]
struct CredentialsElement {
    #[serde(rename = "AccessKeyId")]
    access_key_id: String,
    #[serde(rename = "SecretAccessKey")]
    secret_access_key: String,
    #[serde(rename = "SessionToken")]
    session_token: String,
}

/// Parse STS XML response to extract credentials.
fn parse_credentials(xml: &str) -> Result<AwsCredentials, Error> {
    let response: StsResponse = quick_xml::de::from_str(xml)
        .map_err(|e| Error::Aws(format!("Failed to parse STS response: {e}")))?;

    Ok(AwsCredentials {
        access_key_id: response.result.credentials.access_key_id,
        secret_access_key: response.result.credentials.secret_access_key,
        session_token: Some(response.result.credentials.session_token),
    })
}

/// Sign an AWS request using Signature Version 4.
fn sign_request(
    method: &str,
    url: &str,
    body: &str,
    region: &str,
    creds: &AwsCredentials,
) -> Result<Vec<(String, String)>, Error> {
    let credentials = Credentials::new(
        &creds.access_key_id,
        &creds.secret_access_key,
        creds.session_token.clone(),
        None,
        "k8s-cloud-tagger",
    );
    let identity: Identity = credentials.into();

    let settings = SigningSettings::default();

    let params = v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name("ec2")
        .time(SystemTime::now())
        .settings(settings)
        .build()
        .map_err(|e| Error::Aws(format!("Failed to build signing params: {e}")))?
        .into();

    let signable = SignableRequest::new(
        method,
        url,
        std::iter::empty::<(&str, &str)>(),
        SignableBody::Bytes(body.as_bytes()),
    )
    .map_err(|e| Error::Aws(format!("Failed to create signable request: {e}")))?;

    let (instructions, _signature) = sign(signable, &params)
        .map_err(|e| Error::Aws(format!("Failed to sign request: {e}")))?
        .into_parts();
    // _signature is the raw signature string, but instructions.headers() already
    // contains the complete Authorization header we need, so we discard it.

    let headers: Vec<(String, String)> = instructions
        .headers()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    Ok(headers)
}

pub struct AwsClient {
    http: Client,
    role_arn: String,
    token_file: String,
    region: String,
    account_id: String,
    role_session_name: String,
}

impl AwsClient {
    pub fn new() -> Result<Self, Error> {
        let role_arn =
            std::env::var("AWS_ROLE_ARN").map_err(|_| Error::Aws("AWS_ROLE_ARN not set".into()))?;
        let token_file = std::env::var("AWS_WEB_IDENTITY_TOKEN_FILE")
            .map_err(|_| Error::Aws("AWS_WEB_IDENTITY_TOKEN_FILE not set".into()))?;
        let region =
            std::env::var("AWS_REGION").map_err(|_| Error::Aws("AWS_REGION not set".into()))?;

        // Parse account ID from role ARN: arn:aws:iam::ACCOUNT_ID:role/name
        let account_id = role_arn
            .split(':')
            .nth(4)
            .ok_or_else(|| Error::Aws(format!("Invalid AWS_ROLE_ARN format: {}", role_arn)))?
            .to_string();

        // Use pod name (HOSTNAME) as session name for CloudTrail visibility
        let role_session_name =
            std::env::var("HOSTNAME").unwrap_or_else(|_| "k8s-cloud-tagger".to_string());

        Ok(Self {
            http: http_client()?,
            role_arn,
            token_file,
            region,
            account_id,
            role_session_name,
        })
    }

    async fn credentials(&self) -> Result<AwsCredentials, Error> {
        let token = std::fs::read_to_string(&self.token_file)
            .map_err(|e| Error::Aws(format!("Failed to read {}: {e}", self.token_file)))?;

        let url = format!("https://sts.{}.amazonaws.com/", self.region);

        let resp = self
            .http
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&[
                ("Action", "AssumeRoleWithWebIdentity"),
                ("Version", "2011-06-15"),
                ("RoleArn", &self.role_arn),
                ("RoleSessionName", &self.role_session_name),
                ("WebIdentityToken", token.trim()),
            ])
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            return Err(Error::Aws(format!("STS error ({}): {}", status, body)));
        }

        parse_credentials(&body)
    }

    async fn create_tags(
        &self,
        disk: &AwsDisk,
        tags: &BTreeMap<String, String>,
    ) -> Result<(), Error> {
        let url = disk.endpoint();

        // Build query parameters using owned Strings
        let mut params: Vec<(String, String)> = vec![
            ("Action".to_string(), "CreateTags".to_string()),
            ("Version".to_string(), "2016-11-15".to_string()),
            ("ResourceId.1".to_string(), disk.volume_id.clone()),
        ];

        for (i, (key, value)) in tags.iter().enumerate() {
            let n = i + 1;
            params.push((format!("Tag.{n}.Key"), key.clone()));
            params.push((format!("Tag.{n}.Value"), value.clone()));
        }

        let body = serde_urlencoded::to_string(&params)
            .map_err(|e| Error::Aws(format!("Failed to encode tags: {e}")))?;

        let creds = self.credentials().await?;
        let signed_headers = sign_request("POST", &url, &body, &disk.region, &creds)?;

        let mut request = self
            .http
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body);

        for (key, value) in signed_headers {
            request = request.header(key, value);
        }

        let resp = request.send().await?;
        let status = resp.status();

        if !status.is_success() {
            let text = resp.text().await?;
            return Err(Error::Aws(format!(
                "EC2 CreateTags error ({}): {}",
                status, text
            )));
        }

        tracing::debug!(
            disk = %disk.volume_id,
            tags = ?tags,
            "AWS: tags created"
        );

        Ok(())
    }
}

#[async_trait]
impl CloudClient for AwsClient {
    fn provider_name(&self) -> &'static str {
        "aws"
    }

    async fn set_tags(&self, resource_id: &str, labels: &Labels) -> Result<(), Error> {
        let disk = AwsDisk::parse(resource_id, &self.region, &self.account_id)
            .ok_or_else(|| Error::CloudApi(format!("Invalid AWS volume ID: {resource_id}")))?;

        let sanitised = sanitise_tags(labels);

        self.create_tags(&disk, &sanitised).await?;

        tracing::debug!(
            disk = %resource_id,
            tags = ?sanitised,
            "AWS: tags created"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_volume_id() {
        let disk = AwsDisk::parse("vol-0123456789cafe0", "us-east-1", "123456789012").unwrap();
        assert_eq!(disk.region, "us-east-1");
        assert_eq!(disk.volume_id, "vol-0123456789cafe0");
        assert_eq!(
            disk.arn,
            "arn:aws:ebs:us-east-1:123456789012:volume/vol-0123456789cafe0"
        );
        assert_eq!(disk.endpoint(), "https://ec2.us-east-1.amazonaws.com/");
    }

    #[test]
    fn parse_different_region() {
        let disk = AwsDisk::parse("vol-abc123", "eu-west-2", "999999999999").unwrap();
        assert_eq!(disk.region, "eu-west-2");
        assert_eq!(disk.volume_id, "vol-abc123");
        assert_eq!(
            disk.arn,
            "arn:aws:ebs:eu-west-2:999999999999:volume/vol-abc123"
        );
        assert_eq!(disk.endpoint(), "https://ec2.eu-west-2.amazonaws.com/");
    }

    #[test]
    fn parse_empty_volume_id() {
        assert!(AwsDisk::parse("", "us-east-1", "123456789012").is_none());
    }

    #[test]
    fn sanitise_key_replaces_disallowed() {
        assert_eq!(
            sanitise_aws_tag_key("app.kubernetes.io/name"),
            Some("app.kubernetes.io-name".to_string())
        );
        assert_eq!(
            sanitise_aws_tag_key("key<with>bad%chars"),
            Some("key-with-bad-chars".to_string())
        );
        assert_eq!(
            sanitise_aws_tag_key("env/production"),
            Some("env-production".to_string())
        );
    }

    #[test]
    fn sanitise_key_truncates() {
        let long = "a".repeat(200);
        assert_eq!(sanitise_aws_tag_key(&long).unwrap().len(), 128);
    }

    #[test]
    fn sanitise_key_skips_aws_prefix() {
        assert!(sanitise_aws_tag_key("aws:something").is_none());
        assert!(sanitise_aws_tag_key("aws:created-by").is_none());
        assert_eq!(
            sanitise_aws_tag_key("my-aws-tag"),
            Some("my-aws-tag".to_string())
        );
        assert_eq!(
            sanitise_aws_tag_key("created-by-aws"),
            Some("created-by-aws".to_string())
        );
    }

    #[test]
    fn sanitise_value_truncates() {
        let long = "v".repeat(300);
        assert_eq!(sanitise_aws_tag_value(&long).len(), 256);
    }

    #[test]
    fn sanitise_value_preserves_case() {
        assert_eq!(sanitise_aws_tag_value("Production"), "Production");
        assert_eq!(sanitise_aws_tag_value("Team"), "Team");
    }

    #[test]
    fn sanitise_tags_full() {
        let mut labels = BTreeMap::new();
        labels.insert("app.kubernetes.io/name".to_string(), "frontend".to_string());
        labels.insert("env".to_string(), "production".to_string());
        labels.insert("aws:special".to_string(), "skip-me".to_string());

        let result = sanitise_tags(&labels);
        assert_eq!(result["app.kubernetes.io-name"], "frontend");
        assert_eq!(result["env"], "production");
        assert!(!result.contains_key("aws:special"));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn parse_credentials_valid_xml() {
        let xml = r#"<AssumeRoleWithWebIdentityResponse>
            <AssumeRoleWithWebIdentityResult>
                <Credentials>
                    <AccessKeyId>ASIA1234567890</AccessKeyId>
                    <SecretAccessKey>wJalrXUtnFEMI/K7MDENG/bPxRfiCY1234567890</SecretAccessKey>
                    <SessionToken>FwoGZXIvYXdzEBYaDK1234567890</SessionToken>
                </Credentials>
            </AssumeRoleWithWebIdentityResult>
        </AssumeRoleWithWebIdentityResponse>"#;

        let creds = parse_credentials(xml).unwrap();
        assert_eq!(creds.access_key_id, "ASIA1234567890");
        assert_eq!(
            creds.secret_access_key,
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCY1234567890"
        );
        assert_eq!(
            creds.session_token,
            Some("FwoGZXIvYXdzEBYaDK1234567890".to_string())
        );
    }

    #[test]
    fn parse_credentials_invalid_xml() {
        let xml = "not valid xml";
        assert!(parse_credentials(xml).is_err());
    }

    #[test]
    fn parse_credentials_missing_fields() {
        let xml = r#"<AssumeRoleWithWebIdentityResponse>
            <AssumeRoleWithWebIdentityResult>
                <Credentials>
                    <AccessKeyId>ASIA123</AccessKeyId>
                </Credentials>
            </AssumeRoleWithWebIdentityResult>
        </AssumeRoleWithWebIdentityResponse>"#;

        assert!(parse_credentials(xml).is_err());
    }

    #[test]
    fn sign_request_generates_headers() {
        let creds = AwsCredentials {
            access_key_id: "ASIA123".to_string(),
            secret_access_key: "secret123".to_string(),
            session_token: Some("token123".to_string()),
        };

        let headers = sign_request(
            "POST",
            "https://ec2.us-east-1.amazonaws.com/",
            "Action=CreateTags&Version=2016-11-15",
            "us-east-1",
            &creds,
        )
        .unwrap();

        // Should generate Authorization and X-Amz-Date headers
        let header_names: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
        assert!(header_names.contains(&"authorization"));
        assert!(header_names.contains(&"x-amz-date"));
        assert!(header_names.contains(&"x-amz-security-token"));
    }
}
