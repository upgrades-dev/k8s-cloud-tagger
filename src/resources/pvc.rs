use crate::error::Error;
use crate::traits::{CloudProvider, CloudResource, CloudTaggable};
use k8s_openapi::api::core::v1::{PersistentVolume, PersistentVolumeClaim};
use kube::{Api, Client};

impl CloudTaggable for PersistentVolumeClaim {
    fn resolve_cloud_resource(
        &self,
        client: &Client,
    ) -> impl Future<Output = Result<Option<CloudResource>, Error>> + Send {
        let pv_name = self.spec.as_ref().and_then(|s| s.volume_name.clone());
        let labels = self.metadata.labels.clone().unwrap_or_default();
        let client = client.clone();

        async move {
            let Some(pv_name) = pv_name else {
                // The claim doesn't have a PV associated with it yet.
                return Ok(None);
            };

            let pvs: Api<PersistentVolume> = Api::all(client);
            let pv = pvs.get(&pv_name).await?;

            let Some(resource_id) = extract_resource_id(&pv) else {
                tracing::debug!(pv = %pv_name, "No supported volume source found");
                return Ok(None);
            };

            tracing::debug!(%resource_id, "Found volume");

            Ok(Some(CloudResource {
                provider: CloudProvider::NoOneKnows,
                resource_id,
                labels,
            }))
        }
    }
}

fn extract_resource_id(pv: &PersistentVolume) -> Option<String> {
    let spec = pv.spec.as_ref()?;

    // CSI is the most common and modern.
    if let Some(csi) = &spec.csi {
        return Some(csi.volume_handle.clone());
    }

    // Google Compute Engine Persistent Disk (found on older GKE clusters)
    if let Some(pd_name) = &spec.gce_persistent_disk {
        return Some(pd_name.pd_name.clone());
    }

    None
}
