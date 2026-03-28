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
}
