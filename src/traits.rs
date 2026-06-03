use crate::error::Error;
use kube::{Client, Resource};
use std::collections::BTreeMap;
use std::future::Future;
use std::str::FromStr;

/// A resolved cloud resource ready for tagging.
///
/// This is the cloud-side sibling of a Kubernetes resource.
/// For example, an EBS volume on AWS corresponds to a Kubernetes PVC/PV.
#[derive(Debug, Clone)]
pub struct CloudResource {
    /// The cloud provider that owns this resource.
    pub provider: CloudProvider,
    /// Provider-specific resource identifier (e.g. `vol-0abc123`).
    pub resource_id: String,
    /// Labels to propagate from Kubernetes to the cloud resource.
    pub labels: BTreeMap<String, String>,
}

/// Supported cloud providers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CloudProvider {
    /// For testing. Always succeeds without calling any real cloud API.
    Mock,
    Aws,
    Azure,
    Gcp,
    /// Real volume source but not a recognised cloud provider (e.g. hostPath,
    /// local-path-provisioner, or an unknown CSI driver). The resource ID is
    /// resolved, but no cloud API calls will be made.
    Other,
}

impl std::fmt::Display for CloudProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudProvider::Mock => write!(f, "Mock"),
            CloudProvider::Aws => write!(f, "AWS"),
            CloudProvider::Azure => write!(f, "Azure"),
            CloudProvider::Gcp => write!(f, "GCP"),
            CloudProvider::Other => write!(f, "Other"),
        }
    }
}

impl FromStr for CloudProvider {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "MOCK" => Ok(CloudProvider::Mock),
            "AWS" => Ok(CloudProvider::Aws),
            "AZURE" => Ok(CloudProvider::Azure),
            "GCP" => Ok(CloudProvider::Gcp),
            _ => Err(format!("invalid cloud provider: {}", s)),
        }
    }
}

/// Any Kubernetes resource that can propagate labels to a cloud resource
pub trait CloudTaggable: Resource<DynamicType = ()> + Clone + Send + Sync + 'static {
    /// Resolve the cloud resource (may require fetching intermediate resources)
    fn resolve_cloud_resource(
        &self,
        client: &Client,
    ) -> impl Future<Output = Result<Option<CloudResource>, Error>> + Send;
}
