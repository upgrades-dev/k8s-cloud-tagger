/// A parsed AWS EBS volume ARN.
///
/// The CSI volume handle for `ebs.csi.aws.com` is the full ARN:
///   `arn:aws:ebs:us-east-1:123456789012:volume/vol-0123456789cafe0`
pub struct AwsDisk {
    pub region: String,
    pub account_id: String,
    pub volume_id: String,
}

impl AwsDisk {
    /// Parse an EBS volume ARN.
    ///
    /// Expected format:
    ///   `arn:aws:ebs:{region}:{account}:volume/{volume-id}`
    pub fn parse(arn: &str) -> Option<Self> {
        let parts: Vec<&str> = arn.split(':').collect();
        // Expected: ["arn", "aws", "ebs", region, account, "volume/vol-xxx"]
        if parts.len() != 6 {
            return None;
        }

        if parts[0] != "arn" || parts[1] != "aws" || parts[2] != "ebs" {
            return None;
        }

        let volume_part = parts[5];
        if !volume_part.starts_with("volume/") {
            return None;
        }

        let volume_id = &volume_part[7..]; // Skip "volume/"
        if volume_id.is_empty() {
            return None;
        }

        Some(Self {
            region: parts[3].to_string(),
            account_id: parts[4].to_string(),
            volume_id: volume_id.to_string(),
        })
    }

    /// Build the EC2 endpoint URL for this volume's region.
    pub fn endpoint(&self) -> String {
        format!("https://ec2.{}.amazonaws.com/", self.region)
    }
}

use std::collections::BTreeMap;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_arn() {
        let disk = AwsDisk::parse("arn:aws:ebs:us-east-1:123456789012:volume/vol-0123456789cafe0")
            .unwrap();
        assert_eq!(disk.region, "us-east-1");
        assert_eq!(disk.account_id, "123456789012");
        assert_eq!(disk.volume_id, "vol-0123456789cafe0");
        assert_eq!(disk.endpoint(), "https://ec2.us-east-1.amazonaws.com/");
    }

    #[test]
    fn parse_different_region() {
        let disk = AwsDisk::parse("arn:aws:ebs:eu-west-2:999999999999:volume/vol-abc123").unwrap();
        assert_eq!(disk.region, "eu-west-2");
        assert_eq!(disk.endpoint(), "https://ec2.eu-west-2.amazonaws.com/");
    }

    #[test]
    fn parse_invalid_too_few_parts() {
        assert!(AwsDisk::parse("arn:aws:ebs:us-east-1:123456789012").is_none());
    }

    #[test]
    fn parse_invalid_too_many_parts() {
        assert!(
            AwsDisk::parse("arn:aws:ebs:us-east-1:123456789012:volume:extra:vol-123").is_none()
        );
    }

    #[test]
    fn parse_invalid_prefix() {
        assert!(AwsDisk::parse("arn:aws:s3:us-east-1:123456789012:volume/vol-123").is_none());
        assert!(AwsDisk::parse("urn:aws:ebs:us-east-1:123456789012:volume/vol-123").is_none());
    }

    #[test]
    fn parse_invalid_missing_volume_prefix() {
        assert!(AwsDisk::parse("arn:aws:ebs:us-east-1:123456789012:vol-123").is_none());
    }

    #[test]
    fn parse_invalid_empty_volume_id() {
        assert!(AwsDisk::parse("arn:aws:ebs:us-east-1:123456789012:volume/").is_none());
    }

    #[test]
    fn parse_invalid_empty() {
        assert!(AwsDisk::parse("").is_none());
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
}
