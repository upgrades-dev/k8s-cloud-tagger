use crate::traits::CloudProvider;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

const DEFAULT_PROBE_ADDR: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 8080));

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
    pub fn from_env() -> Self {
        Self {
            requeue_success: parse_duration_env("REQUEUE_SUCCESS_SECS", 300),
            requeue_not_ready: parse_duration_env("REQUEUE_NOT_READY_SECS", 30),
            requeue_error: parse_duration_env("REQUEUE_ERROR_SECS", 60),
            probe_addr: parse_addr_env("PROBE_ADDR", DEFAULT_PROBE_ADDR),
            cloud_provider: parse_cloud_provider_env("CLOUD_PROVIDER", CloudProvider::Mock),
        }
    }
}

fn parse_duration_env(key: &str, default: u64) -> Duration {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(default))
}

fn parse_addr_env(key: &str, default: SocketAddr) -> SocketAddr {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn parse_cloud_provider_env(key: &str, default: CloudProvider) -> CloudProvider {
    match std::env::var(key) {
        Ok(val) => val.parse::<CloudProvider>().unwrap_or_else(|err| {
            tracing::warn!(%key, %val, %err, "invalid value, using default '{default}'");
            default
        }),
        Err(_) => {
            tracing::info!(%key, "not set, using default '{default}'");
            default
        }
    }
}
