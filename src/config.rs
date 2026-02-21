use crate::traits::CloudProvider;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::Path;
use std::time::Duration;

const DEFAULT_PROBE_ADDR: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 8080));
const DEFAULT_CONFIG_PATH: &str = "/etc/k8s-cloud-tagger/config.yaml";

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileConfig {
    cloud_provider: String,
    requeue: FileRequeueConfig,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileRequeueConfig {
    success: String,
    not_ready: String,
    error: String,
}

pub struct Config {
    pub requeue_success: Duration,
    pub requeue_not_ready: Duration,
    pub requeue_error: Duration,
    pub probe_addr: SocketAddr,
    pub cloud_provider: CloudProvider,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            requeue_success: Duration::from_secs(300),
            requeue_not_ready: Duration::from_secs(30),
            requeue_error: Duration::from_secs(60),
            probe_addr: DEFAULT_PROBE_ADDR,
            cloud_provider: CloudProvider::Mock,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());
        Self::from_file(path)
    }
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let fc: FileConfig = serde_yaml::from_str(&raw).map_err(|e| e.to_string())?;
        Ok(Self {
            requeue_success: parse_duration_str(&fc.requeue.success)?,
            requeue_not_ready: parse_duration_str(&fc.requeue.not_ready)?,
            requeue_error: parse_duration_str(&fc.requeue.error)?,
            probe_addr: DEFAULT_PROBE_ADDR,
            cloud_provider: fc.cloud_provider.parse()?,
        })
    }
}

fn parse_duration_str(s: &str) -> Result<Duration, String> {
    if let Some(v) = s.strip_suffix('m') {
        v.parse::<u64>()
            .map(|n| Duration::from_secs(n * 60))
            .map_err(|e| e.to_string())
    } else if let Some(v) = s.strip_suffix('s') {
        v.parse::<u64>()
            .map(Duration::from_secs)
            .map_err(|e| e.to_string())
    } else {
        Err(format!(
            "unrecognised duration format: '{s}' (expected e.g. '5m' or '30s')"
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn test_from_file_parses_correctly() {
        let yaml = "\
cloudProvider: \"GCP\"
requeue:
  success: \"5m\"
  notReady: \"30s\"
  error: \"1m\"
";
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", yaml).unwrap();

        let cfg = Config::from_file(file.path()).unwrap();

        assert_eq!(cfg.requeue_success, Duration::from_secs(300));
        assert_eq!(cfg.requeue_not_ready, Duration::from_secs(30));
        assert_eq!(cfg.requeue_error, Duration::from_secs(60));
        assert_eq!(cfg.probe_addr, DEFAULT_PROBE_ADDR);
        assert!(matches!(
            cfg.cloud_provider,
            crate::traits::CloudProvider::Gcp
        ));
    }

    #[test]
    fn test_from_file_missing_returns_err() {
        let result = Config::from_file("/nonexistent/path/config.yaml");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_duration_str() {
        assert_eq!(parse_duration_str("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration_str("30s").unwrap(), Duration::from_secs(30));
        assert!(parse_duration_str("bad").is_err());
    }
}
