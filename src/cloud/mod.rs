mod mock;

pub use mock::MockClient;

use crate::error::Error;
use crate::metrics::API_CALL_DURATION;
use async_trait::async_trait;
use std::collections::BTreeMap;

pub type Labels = BTreeMap<String, String>;

#[async_trait]
pub trait CloudClient: Send + Sync {
    fn provider_name(&self) -> &'static str;

    async fn set_tags(&self, resource_id: &str, labels: &Labels) -> Result<(), Error>;
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
