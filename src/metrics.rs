use prometheus::{HistogramVec, IntCounterVec, IntGaugeVec};
use prometheus::{register_histogram_vec, register_int_counter_vec, register_int_gauge_vec};
use std::sync::LazyLock;

/// Total reconciliations by resource and outcome (success/error)
pub static RECONCILE_COUNT: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "reconcile_total",
        "Total reconciliations",
        &["resource", "outcome"]
    )
    .unwrap()
});

/// Duration of reconciliations in seconds
pub static RECONCILE_DURATION: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        "reconcile_duration_seconds",
        "Reconcile duration in seconds",
        &["resource"],
        vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0] // buckets for histogram quantile
    )
    .unwrap()
});

/// Currently active reconciliations
pub static RECONCILE_ACTIVE: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!("reconcile_active", "Active reconciliations", &["resource"]).unwrap()
});

/// Errors by resource and error type
pub static ERRORS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec!(
        "errors_total",
        "Total errors by type",
        &["resource", "error_type"]
    )
    .unwrap()
});

/// Duration of external API calls in seconds
pub static _API_CALL_DURATION: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec!(
        "api_call_duration_seconds",
        "Duration of external API calls",
        &["provider", "operation"],
        vec![0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0] // buckets for histogram quantile
    )
    .unwrap()
});
