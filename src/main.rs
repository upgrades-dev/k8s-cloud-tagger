mod error;
mod reconciler;
mod resources;
mod traits;
use crate::reconciler::Context;
use crate::reconciler::{error_policy, reconcile};
use futures::StreamExt;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use kube::runtime::Controller;
use kube::runtime::watcher::Config;
use kube::{Api, Client};
use std::sync::Arc;
use tracing_subscriber::fmt::format::FmtSpan;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .init();

    tracing::info!("Starting k8s-cloud-tagger");

    // Create Kubernetes client (uses ~/.kube/config or in-cluster config)
    let client = Client::try_default().await?;
    let ctx = Arc::new(Context {
        client: client.clone(),
    });

    let pvcs: Api<PersistentVolumeClaim> = Api::all(client);

    Controller::new(pvcs, Config::default())
        .run(reconcile, error_policy, ctx)
        .for_each(|_| async move {})
        .await;

    Ok(())
}
