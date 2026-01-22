use crate::error::Error;
use crate::traits::CloudTaggable;
use kube::runtime::controller::Action;
use kube::{Client, ResourceExt};
use std::sync::Arc;
use std::time::Duration;

pub struct Context {
    pub client: Client,
}

pub async fn reconcile<T>(resource: Arc<T>, ctx: Arc<Context>) -> Result<Action, Error>
where
    T: CloudTaggable + ResourceExt,
{
    let name = resource.name_any();
    let namespace = resource.namespace().unwrap_or_default();
    // TODO(afharvey): log the kind of resource

    tracing::debug!("Reconciling resource {}/{}", namespace, name);

    // Resolve the cloud resource (may need intermediate lookups)
    let cloud_resource = resource.resolve_cloud_resource(&ctx.client).await?;

    // TODO(afharvey): decide durations, they should be configurable
    match cloud_resource {
        Some(cr) => {
            tracing::info!(
                provider = ?cr.provider,
                resource_id = %cr.resource_id,
                labels = ?cr.labels,
                "Ready to tag cloud resource"
            );

            Ok(Action::requeue(Duration::from_secs(300)))
        }
        None => Ok(Action::requeue(Duration::from_secs(30))),
    }
}

pub fn error_policy<T>(_resource: Arc<T>, error: &Error, _ctx: Arc<Context>) -> Action
where
    T: CloudTaggable,
{
    tracing::error!(%error, "Reconciliation error");
    Action::requeue(Duration::from_secs(60))
}
