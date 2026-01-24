use std::time::Duration;

pub struct Config {
    pub requeue_success: Duration,
    pub requeue_not_ready: Duration,
    pub requeue_error: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            requeue_success: Duration::from_secs(300),
            requeue_not_ready: Duration::from_secs(30),
            requeue_error: Duration::from_secs(60),
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            requeue_success: parse_duration_env("REQUEUE_SUCCESS_SECS", 300),
            requeue_not_ready: parse_duration_env("REQUEUE_NOT_READY_SECS", 30),
            requeue_error: parse_duration_env("REQUEUE_ERROR_SECS", 60),
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
