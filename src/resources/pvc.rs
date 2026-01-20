use k8s_openapi::api::core::v1::{PersistentVolume, PersistentVolumeClaim};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{Api, Client};
use crate::traits::{CloudProvider, CloudResource, CloudTaggable, ResolveResult};

impl CloudTaggable for PersistentVolumeClaim {
    fn resolve_cloud_resource(
        &self,
        client: &Client,
    ) -> impl Future<Output=ResolveResult> + Send {
        let pv_name = self
            .spec
            .as_ref()
            .and_then(|s| s.volume_name.clone());
        let labels = self.metadata.labels.clone().unwrap_or_default();
        let client = client.clone();

        async move {
            let Some(pv_name) = pv_name else {
                return Ok(None);
            };

            let pvs: Api<PersistentVolume> = Api::all(client);
            let pv = pvs.get(&pv_name).await?;

            Ok(Some(CloudResource {
                provider: CloudProvider::Linode,
                resource_id: pv_name,
                labels,
            }))
        }
    }

    fn metadata(&self) -> &ObjectMeta {
        &self.metadata
    }
}