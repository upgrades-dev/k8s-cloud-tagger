use crate::cloud::{CloudClient, MeteredClient};
use crate::config::Config;
use crate::error::Error;
use crate::metrics::{ERRORS, RECONCILE_ACTIVE, RECONCILE_COUNT, RECONCILE_DURATION, labels};
use crate::traits::CloudTaggable;
use kube::runtime::controller::Action;
use kube::{Client, Resource, ResourceExt};
use std::sync::Arc;
use std::time::Instant;

pub struct Context<C: CloudClient> {
    pub client: Client,
    pub config: Config,
    pub cloud: MeteredClient<C>,
}

pub async fn reconcile<T, C>(resource: Arc<T>, ctx: Arc<Context<C>>) -> Result<Action, Error>
where
    T: CloudTaggable + ResourceExt + Resource<DynamicType = ()>,
    C: CloudClient,
{
    let start = Instant::now();
    let (kind, namespace, name) = resource_ref(resource.clone().as_ref());

    RECONCILE_ACTIVE.with_label_values(&[&kind]).inc();
    tracing::debug!(%kind, %namespace, %name, "Reconciling");

    let result = do_reconcile(resource, ctx.as_ref(), &kind, &namespace, &name).await;

    RECONCILE_ACTIVE.with_label_values(&[&kind]).dec();
    RECONCILE_DURATION
        .with_label_values(&[&kind.as_str()])
        .observe(start.elapsed().as_secs_f64());

    match &result {
        Ok(_) => {
            RECONCILE_COUNT
                .with_label_values(&[kind.as_str(), labels::SUCCESS])
                .inc();
        }
        Err(e) => {
            RECONCILE_COUNT
                .with_label_values(&[kind.as_str(), labels::ERROR])
                .inc();
            ERRORS
                .with_label_values(&[kind.as_str(), e.metric_label()])
                .inc();
        }
    }

    result
}

async fn do_reconcile<T, C>(
    resource: Arc<T>,
    ctx: &Context<C>,
    kind: &str,
    namespace: &str,
    name: &str,
) -> Result<Action, Error>
where
    T: CloudTaggable + ResourceExt + Resource<DynamicType = ()>,
    C: CloudClient,
{
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

            // Calls the cloud provider API and sets tags on the resource.
            ctx.cloud.set_tags(&cr.resource_id, &cr.labels).await?;

            Ok(Action::requeue(ctx.config.requeue_success))
        }
        None => {
            tracing::debug!(%kind, %namespace, %name, "Not ready");
            Ok(Action::requeue(ctx.config.requeue_not_ready))
        }
    }
}

pub fn error_policy<T, C>(resource: Arc<T>, error: &Error, ctx: Arc<Context<C>>) -> Action
where
    T: CloudTaggable + ResourceExt + Resource<DynamicType = ()>,
    C: CloudClient,
{
    let (kind, namespace, name) = resource_ref(resource.as_ref());
    tracing::error!(%kind, %namespace, %name, %error, "Reconciliation error");
    Action::requeue(ctx.config.requeue_error)
}

fn resource_ref<T>(resource: &T) -> (String, String, String)
where
    T: Resource<DynamicType = ()> + ResourceExt,
{
    (
        T::kind(&()).into_owned().to_lowercase(),
        resource
            .namespace()
            .unwrap_or_else(|| "<cluster>".to_string()),
        resource.name_any(),
    )
}
