use crate::cloud::{CloudClient, MeteredClient};
use crate::config::Config;
use crate::error::Error;
use crate::metrics::{ERRORS, RECONCILE_ACTIVE, RECONCILE_COUNT, RECONCILE_DURATION, labels};
use crate::traits::CloudTaggable;
use kube::runtime::controller::Action;
use kube::runtime::events::{Event, EventType, Recorder, Reporter};
use kube::{Client, Resource, ResourceExt};
use std::sync::Arc;
use std::time::Instant;

/// Shared state for the reconciler, passed to every reconciliation call.
pub struct Context<C: CloudClient> {
    /// Kubernetes API client.
    pub client: Client,
    /// Controller configuration (requeue intervals, etc.).
    pub config: Config,
    /// Cloud provider API client with metrics instrumentation.
    pub cloud: MeteredClient<C>,
    /// Event reporter identity (controller name and pod instance).
    pub reporter: Reporter,
}

/// Main reconcile entry point, called by the kube-rs controller runtime.
pub async fn reconcile<T, C>(resource: Arc<T>, ctx: Arc<Context<C>>) -> Result<Action, Error>
where
    T: CloudTaggable + ResourceExt,
    C: CloudClient,
{
    let start = Instant::now();
    let (kind, namespace, name) = resource_ref(resource.as_ref());

    RECONCILE_ACTIVE.with_label_values(&[&kind]).inc();
    tracing::debug!(%kind, %namespace, %name, "Reconciling");

    let result = do_reconcile(resource.as_ref(), ctx.as_ref(), &kind, &namespace, &name).await;

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
    resource: &T,
    ctx: &Context<C>,
    kind: &str,
    namespace: &str,
    name: &str,
) -> Result<Action, Error>
where
    T: CloudTaggable + ResourceExt,
    C: CloudClient,
{
    // Skip resources that are being deleted.
    if resource.meta().deletion_timestamp.is_some() {
        tracing::debug!(%kind, %namespace, %name, "Resource is being deleted, skipping");
        return Ok(Action::await_change());
    }

    // Resolve the cloud resource (may need intermediate lookups)
    let cloud_resource = resource.resolve_cloud_resource(&ctx.client).await?;

    match cloud_resource {
        Some(cr) => {
            tracing::info!(
                %kind, %namespace, %name,
                provider = %cr.provider,
                resource_id = %cr.resource_id,
                labels = ?cr.labels,
                "Ready to tag cloud resource"
            );

            // Calls the cloud provider API and sets tags on the resource.
            ctx.cloud.set_tags(&cr.resource_id, &cr.labels).await?;

            // Publish a Kubernetes event explaining that we successfully tagged the resource.
            let recorder = Recorder::new(ctx.client.clone(), ctx.reporter.clone());

            // Events are best-effort, don't trigger reconciliation again.
            if let Err(e) = recorder
                .publish(
                    &Event {
                        type_: EventType::Normal,
                        reason: "Tagged".into(),
                        note: Some(format!(
                            "Tagged {} with {} label(s)",
                            cr.resource_id,
                            cr.labels.len(),
                        )),
                        action: "TagCloudResource".into(),
                        secondary: None,
                    },
                    &resource.object_ref(&()),
                )
                .await
            {
                tracing::warn!(%kind, %namespace, %name, %e, "Failed to publish event");
            }

            Ok(Action::requeue(ctx.config.requeue_success))
        }
        None => {
            tracing::debug!(%kind, %namespace, %name, "Not ready");
            Ok(Action::requeue(ctx.config.requeue_not_ready))
        }
    }
}

/// Called by the controller runtime when reconciliation returns an error.
pub fn error_policy<T, C>(resource: Arc<T>, error: &Error, ctx: Arc<Context<C>>) -> Action
where
    T: CloudTaggable + ResourceExt,
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
        T::kind(&()).to_lowercase(),
        resource
            .namespace()
            .unwrap_or_else(|| "<cluster>".to_string()),
        resource.name_any(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{CloudProvider, CloudResource};
    use async_trait::async_trait;
    use bytes::Bytes;
    use jiff::Timestamp;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // =========================================================================
    // Mock cloud client — tracks calls
    // =========================================================================

    #[derive(Clone)]
    struct MockCloud {
        tag_calls: Arc<AtomicUsize>,
        last_resource_id: Arc<Mutex<String>>,
        last_labels: Arc<Mutex<BTreeMap<String, String>>>,
        should_fail: bool,
    }

    impl Default for MockCloud {
        fn default() -> Self {
            Self {
                tag_calls: Arc::new(AtomicUsize::new(0)),
                last_resource_id: Arc::new(Mutex::new(String::new())),
                last_labels: Arc::new(Mutex::new(BTreeMap::new())),
                should_fail: false,
            }
        }
    }

    #[async_trait]
    impl CloudClient for MockCloud {
        fn provider_name(&self) -> &'static str {
            "mock"
        }

        async fn set_tags(
            &self,
            resource_id: &str,
            labels: &BTreeMap<String, String>,
        ) -> Result<(), Error> {
            self.tag_calls.fetch_add(1, Ordering::Relaxed);
            *self.last_resource_id.lock().unwrap() = resource_id.to_string();
            *self.last_labels.lock().unwrap() = labels.clone();
            if self.should_fail {
                return Err(Error::CloudApi("mock failure".into()));
            }
            Ok(())
        }
    }

    // =========================================================================
    // Mock resource — implements Resource + CloudTaggable with controlled behavior
    // =========================================================================

    #[derive(Clone)]
    struct MockResource {
        meta: ObjectMeta,
        cloud_resource: Option<CloudResource>,
        resolve_error: bool,
    }

    impl Resource for MockResource {
        type DynamicType = ();
        type Scope = k8s_openapi::NamespaceResourceScope;

        fn kind(_: &()) -> std::borrow::Cow<'_, str> {
            "MockResource".into()
        }
        fn group(_: &()) -> std::borrow::Cow<'_, str> {
            "upgrades.dev".into()
        }
        fn version(_: &()) -> std::borrow::Cow<'_, str> {
            "v1".into()
        }
        fn plural(_: &()) -> std::borrow::Cow<'_, str> {
            "mockresources".into()
        }
        fn meta(&self) -> &ObjectMeta {
            &self.meta
        }
        fn meta_mut(&mut self) -> &mut ObjectMeta {
            &mut self.meta
        }
    }

    impl CloudTaggable for MockResource {
        async fn resolve_cloud_resource(
            &self,
            _client: &Client,
        ) -> Result<Option<CloudResource>, Error> {
            if self.resolve_error {
                return Err(Error::CloudApi("resolve failed".into()));
            }
            Ok(self.cloud_resource.clone())
        }
    }

    // =========================================================================
    // Test helpers
    // =========================================================================

    /// Creates a kube::Client backed by a mock service.
    /// - MockResource ignores the client in resolve_cloud_resource
    /// - Event publish failures are handled gracefully in do_reconcile
    fn mock_client() -> Client {
        let mock_service = tower::service_fn(|_req: http::Request<kube::client::Body>| async {
            Ok::<_, std::convert::Infallible>(
                http::Response::builder()
                    .status(200)
                    .body(kube::client::Body::from(Bytes::from(
                        r#"{"kind":"Status","status":"Success"}"#,
                    )))
                    .unwrap(),
            )
        });
        Client::new(mock_service, "default")
    }

    fn test_ctx(cloud: MockCloud) -> Context<MockCloud> {
        Context {
            client: mock_client(),
            config: Default::default(),
            cloud: MeteredClient::new(cloud),
            reporter: Reporter {
                controller: "test".into(),
                instance: None,
            },
        }
    }

    fn mock_resource(name: &str, cloud_resource: Option<CloudResource>) -> MockResource {
        MockResource {
            meta: ObjectMeta {
                name: Some(name.into()),
                namespace: Some("default".into()),
                ..Default::default()
            },
            cloud_resource,
            resolve_error: false,
        }
    }

    fn sample_cloud_resource() -> CloudResource {
        CloudResource {
            provider: CloudProvider::Mock,
            resource_id: "vol-abc123".into(),
            labels: BTreeMap::from([("upgrades.dev/app".into(), "k8s-cloud-tagger".into())]),
        }
    }

    // =========================================================================
    // Tests
    // =========================================================================

    #[tokio::test]
    async fn tags_cloud_resource_on_match() {
        let cloud = MockCloud::default();
        let calls = cloud.tag_calls.clone();
        let last_id = cloud.last_resource_id.clone();
        let last_labels = cloud.last_labels.clone();
        let ctx = test_ctx(cloud);
        let resource = mock_resource("my-pvc", Some(sample_cloud_resource()));

        let result = do_reconcile(&resource, &ctx, "mockresource", "default", "my-pvc").await;

        assert!(result.is_ok(), "reconcile should succeed");
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "cloud API should be called once"
        );
        assert_eq!(*last_id.lock().unwrap(), "vol-abc123");
        assert_eq!(
            *last_labels.lock().unwrap(),
            BTreeMap::from([("upgrades.dev/app".into(), "k8s-cloud-tagger".into())])
        );
    }

    #[tokio::test]
    async fn skips_tagging_when_not_ready() {
        let cloud = MockCloud::default();
        let calls = cloud.tag_calls.clone();
        let ctx = test_ctx(cloud);
        let resource = mock_resource("pending-pvc", None);

        let result = do_reconcile(&resource, &ctx, "mockresource", "default", "pending-pvc").await;

        assert!(result.is_ok(), "reconcile should succeed (requeue)");
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "cloud API should not be called"
        );
    }

    #[tokio::test]
    async fn propagates_cloud_api_error() {
        let cloud = MockCloud {
            should_fail: true,
            ..Default::default()
        };
        let calls = cloud.tag_calls.clone();
        let ctx = test_ctx(cloud);
        let resource = mock_resource("my-pvc", Some(sample_cloud_resource()));

        let result = do_reconcile(&resource, &ctx, "mockresource", "default", "my-pvc").await;

        assert!(result.is_err(), "reconcile should return error");
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "cloud API should be attempted"
        );
    }

    #[tokio::test]
    async fn propagates_resolve_error() {
        let cloud = MockCloud::default();
        let calls = cloud.tag_calls.clone();
        let ctx = test_ctx(cloud);
        let mut resource = mock_resource("broken-pvc", None);
        resource.resolve_error = true;

        let result = do_reconcile(&resource, &ctx, "mockresource", "default", "broken-pvc").await;

        assert!(result.is_err(), "resolve error should propagate");
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "cloud API should not be called"
        );
    }

    #[tokio::test]
    async fn error_policy_will_requeue() {
        let cloud = MockCloud::default();
        let ctx = Arc::new(test_ctx(cloud));
        let resource = Arc::new(mock_resource("my-pvc", None));
        let error = Error::CloudApi("error from cloud provider".into());

        // Verify error_policy returns without panicking
        let _action = error_policy(resource, &error, ctx);
    }

    #[tokio::test]
    async fn skips_deleted_resource() {
        let cloud = MockCloud::default();
        let calls = cloud.tag_calls.clone();
        let ctx = test_ctx(cloud);
        let mut resource = mock_resource("deleted-pvc", Some(sample_cloud_resource()));
        resource.meta.deletion_timestamp = Some(
            k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(Timestamp::now()),
        );

        let result = do_reconcile(&resource, &ctx, "mockresource", "default", "deleted-pvc").await;

        assert!(result.is_ok(), "reconcile should succeed");
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "cloud API should not be called for deleted resource"
        );
    }
}
