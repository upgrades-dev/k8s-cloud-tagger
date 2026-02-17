mod mock;

pub use mock::MockClient;

use crate::error::Error;
use crate::metrics::API_CALL_DURATION;
use crate::traits::CloudProvider;
use async_trait::async_trait;
use std::collections::BTreeMap;

pub type Labels = BTreeMap<String, String>;

#[async_trait]
pub trait CloudClient: Send + Sync {
    fn provider_name(&self) -> &'static str;

    async fn set_tags(&self, resource_id: &str, labels: &Labels) -> Result<(), Error>;
}

/// Blanket implementation of [`CloudClient`] for boxed trait objects.
///
/// This allows any `Box<dyn CloudClient>` to be used interchangeably wherever a
/// concrete [`CloudClient`] is expected, enabling provider-agnostic usage of
/// cloud clients through dynamic dispatch.
#[async_trait]
impl CloudClient for Box<dyn CloudClient> {
    /// Returns the name of the underlying cloud provider by delegating to the
    /// inner implementation.
    fn provider_name(&self) -> &'static str {
        (**self).provider_name()
    }

    /// Applies the given labels to the specified resource by delegating to the
    /// inner implementation.
    async fn set_tags(&self, resource_id: &str, labels: &Labels) -> Result<(), Error> {
        (**self).set_tags(resource_id, labels).await
    }
}

/// Wrapper which adds metrics to any CloudClient
pub struct MeteredClient<C: CloudClient> {
    inner: C,
}

impl<C: CloudClient> MeteredClient<C> {
    pub fn new(inner: C) -> Self {
        Self { inner }
    }

    pub async fn set_tags(&self, resource_id: &str, labels: &Labels) -> Result<(), Error> {
        let start = std::time::Instant::now();
        let result = self.inner.set_tags(resource_id, labels).await;

        API_CALL_DURATION
            .with_label_values(&[self.inner.provider_name(), "set_tags"])
            .observe(start.elapsed().as_secs_f64());

        result
    }
}

pub async fn create_client(provider: &CloudProvider) -> Result<Box<dyn CloudClient>, Error> {
    match provider {
        CloudProvider::Mock => Ok(Box::new(MockClient::default())),
        CloudProvider::Gcp => Err(Error::NotImplemented),
    }
}
