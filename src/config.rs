use std::time::Duration;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

const DEFAULT_PROBE_ADDR: SocketAddr = SocketAddr::new(
    IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
    8080,
);

pub struct Config {
    pub requeue_success: Duration,
    pub requeue_not_ready: Duration,
    pub requeue_error: Duration,
    pub probe_addr: SocketAddr,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            requeue_success: Duration::from_secs(300),
            requeue_not_ready: Duration::from_secs(30),
            requeue_error: Duration::from_secs(60),
            probe_addr: DEFAULT_PROBE_ADDR,
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
