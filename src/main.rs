mod cloud;
mod config;
mod error;
mod health;
mod metrics;
mod reconciler;
mod resources;
mod traits;

use crate::cloud::{MeteredClient, MockClient};
use crate::reconciler::Context;
use crate::reconciler::{error_policy, reconcile};
use futures::StreamExt;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use kube::runtime::Controller;
use kube::runtime::watcher::Config;
use kube::{Api, Client};
use std::sync::Arc;
use tokio::signal;
use tracing_subscriber::fmt::format::FmtSpan;

macro_rules! controller {
    ($t:ty, $client:expr, $ctx: expr) => {
        Controller::new(Api::<$t>::all($client), Config::default())
            .run(reconcile, error_policy, $ctx.clone())
            .for_each(|_| async move {})
    };
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .init();

    tracing::info!("Starting k8s-cloud-tagger");

    let cfg = config::Config::from_env();
    let probe_addr = cfg.probe_addr;

    let client = Client::try_default().await?;
    let ctx = Arc::new(Context {
        client: client.clone(),
        config: cfg,
        cloud: MeteredClient::new(MockClient::default()),
    });

    let pvc_ctrl = controller!(PersistentVolumeClaim, client, ctx);

    tokio::select! {
        result = health::serve(probe_addr) => result?,
        _ = pvc_ctrl => {}
        _ = signal::ctrl_c() => tracing::debug!("Shutting down"),
    }

    Ok(())
}
