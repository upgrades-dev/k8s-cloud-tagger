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

pub async fn serve(addr: SocketAddr) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz));

    let listener = TcpListener::bind(addr).await?;
    tracing::debug!(%addr, "Health server listening");
    axum::serve(listener, app).await?;

    Ok(())
}
