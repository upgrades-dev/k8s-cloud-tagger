use crate::config::Config;
use crate::error::Error;
use crate::traits::CloudTaggable;
use kube::runtime::controller::Action;
use kube::{Client, Resource, ResourceExt};
use std::borrow::Cow;
use std::sync::Arc;

pub struct Context {
    pub client: Client,
    pub config: Config,
}

pub async fn reconcile<T>(resource: Arc<T>, ctx: Arc<Context>) -> Result<Action, Error>
where
    T: CloudTaggable + ResourceExt + Resource<DynamicType = ()>,
{
    let (kind, namespace, name) = resource_ref(resource.as_ref());
    tracing::debug!(%kind, %namespace, %name, "Reconciling");

    // Resolve the cloud resource (may need intermediate lookups)
    let cloud_resource = resource.resolve_cloud_resource(&ctx.client).await?;

    match cloud_resource {
        Some(cr) => {
            tracing::info!(
                %kind, %namespace, %name,
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

pub fn error_policy<T>(resource: Arc<T>, error: &Error, ctx: Arc<Context>) -> Action
where
    T: CloudTaggable + ResourceExt + Resource<DynamicType = ()>,
{
    let (kind, namespace, name) = resource_ref(resource.as_ref());
    tracing::error!(%kind, %namespace, %name, %error, "Reconciliation error");
    Action::requeue(ctx.config.requeue_error)
}

fn resource_ref<T>(resource: &T) -> (Cow<'_, str>, String, String)
where
    T: Resource<DynamicType = ()> + ResourceExt,
{
    (
        T::kind(&()),
        resource
            .namespace()
            .unwrap_or_else(|| "<cluster>".to_string()),
        resource.name_any(),
    )
}
