use super::{CloudClient, Labels};
use crate::error::Error;
use async_trait::async_trait;
use std::time::Duration;

pub struct MockClient {
    delay: Duration,
}

impl MockClient {
    pub fn new(delay: Duration) -> Self {
        Self { delay }
    }
}

impl Default for MockClient {
    fn default() -> Self {
        Self::new(Duration::from_secs(1))
    }
}

#[async_trait]
impl CloudClient for MockClient {
    fn provider_name(&self) -> &'static str {
        "mock"
    }

    async fn set_tags(&self, resource_id: &str, tags: &Labels) -> Result<(), Error> {
        tracing::debug!(%resource_id, ?tags, "Mock: setting tags");
        // Simulate API latency
        tokio::time::sleep(self.delay).await;
        Ok(())
    }
}
