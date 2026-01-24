use kube::{Client, Resource};
use std::collections::BTreeMap;

use crate::error::Error;

/// The resolved cloud resource ready for tagging.
/// This is something in the cloud provider.
/// It is the sibling to a Kubernetes resource.
/// Like an EBS volume (disk) on AWS is related to a Kubernetes PVC or PV.
pub struct CloudResource {
    pub provider: CloudProvider,
    pub resource_id: String,
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum CloudProvider {
    NoOneKnows, // TODO(afharvey) figure cloud provider out later
                // Linode,     // Also known as Akamai
                // Aws,
                // Azure,
                // Gcp,
}

/// Any Kubernetes resource that can propagate labels to a cloud resource
pub trait CloudTaggable: Resource + Clone + Send + Sync + 'static {
    /// Resolve the cloud resource (may require fetching intermediate resources)
    fn resolve_cloud_resource(
        &self,
        client: &Client,
    ) -> impl Future<Output = Result<Option<CloudResource>, Error>> + Send;
}
