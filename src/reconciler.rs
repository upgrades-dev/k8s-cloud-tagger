use crate::config::Config;
use crate::error::Error;
use crate::traits::CloudTaggable;
use kube::runtime::controller::Action;
use kube::{Client, Resource, ResourceExt};
use std::sync::Arc;

pub struct Context {
    pub client: Client,
    pub config: Config,
}

pub async fn reconcile<T>(resource: Arc<T>, ctx: Arc<Context>) -> Result<Action, Error>
where
    T: CloudTaggable + ResourceExt + Resource<DynamicType = ()>,
{
    let kind = T::kind(&());
    let name = resource.name_any();
    let namespace = resource.namespace().unwrap_or_default();

    tracing::debug!(%kind, %namespace, %name, "Reconciling");

    // Resolve the cloud resource (may need intermediate lookups)
    let cloud_resource = resource.resolve_cloud_resource(&ctx.client).await?;

    match cloud_resource {
        Some(cr) => {
            tracing::info!(
                provider = ?cr.provider,
                resource_id = %cr.resource_id,
                labels = ?cr.labels,
                "Ready to tag cloud resource"
            );
            Ok(Action::requeue(ctx.config.requeue_success))
        }
        None => {
            tracing::debug!(%kind, %namespace, %name, "Not ready");
            Ok(Action::requeue(ctx.config.requeue_not_ready))
        }
    }
}

pub fn error_policy<T>(_resource: Arc<T>, error: &Error, ctx: Arc<Context>) -> Action
where
    T: CloudTaggable,
{
    tracing::error!(%error, "Reconciliation error");
    Action::requeue(ctx.config.requeue_error)
}
