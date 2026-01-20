use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{Client, Resource};
use std::collections::BTreeMap;

use crate::error::Error;

pub type ResolveResult = Result<Option<CloudResource>, Error>;

/// The resolved cloud resource ready for tagging
pub struct CloudResource {
    pub provider: CloudProvider,
    pub resource_id: String,
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum CloudProvider {
    Linode,
    /// Also known as Akamai
    Aws,
    Azure,
    Gcp,
}

/// Any Kubernetes resource that can propagate labels to a cloud resource
pub trait CloudTaggable: Resource + Clone + Send + Sync + 'static {
    /// Resolve the cloud resource (may require fetching intermediate resources)
    fn resolve_cloud_resource(
        &self,
        client: &Client,
    ) -> impl Future<Output = ResolveResult> + Send;

    fn metadata(&self) -> &ObjectMeta;
}
