use axum::{Router, http::StatusCode, routing::get};
use std::net::SocketAddr;
use tokio::net::TcpListener;

/// Liveness probe - is the process alive?
/// Always returns 200 OK.
async fn healthz() -> StatusCode {
    StatusCode::OK
}

/// Readiness probe - can this instance handle traffic?
/// TODO:
///     Return 503 until leader election acquired.
///     https://github.com/upgrades-dev/k8s-cloud-tagger/issues/29
async fn readyz() -> StatusCode {
    StatusCode::OK
}

/// Prometheus metrics endpoint
async fn metrics() -> (StatusCode, String) {
    let encoder = prometheus::TextEncoder::new();
    match encoder.encode_to_string(&prometheus::gather()) {
        Ok(s) => (StatusCode::OK, s),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn serve(addr: SocketAddr) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics));

    let listener = TcpListener::bind(addr).await?;
    tracing::debug!(%addr, "Health server listening");
    axum::serve(listener, app).await?;

    Ok(())
}
