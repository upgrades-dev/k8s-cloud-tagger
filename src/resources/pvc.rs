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
                provider: CloudProvider::Mock,
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

    // hostPath - used by Kind/local-path-provisioner for test environments
    if let Some(host_path) = &spec.host_path {
        return Some(host_path.path.clone());
    }

    let pv_name = pv.metadata.name.as_deref().unwrap_or("<unknown>");
    tracing::warn!(pv = %pv_name, "No supported volume source found (expected CSI, GCE PD, or hostPath)");

    None
}

#[cfg(test)]
mod tests {
    use crate::traits::CloudTaggable;
    use http::{Request, Response, StatusCode};
    use k8s_openapi::api::core::v1::{
        CSIPersistentVolumeSource, PersistentVolume, PersistentVolumeClaim,
        PersistentVolumeClaimSpec, PersistentVolumeSpec,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::Client;
    use kube::client::Body;
    use tower_test::mock;

    fn mock_client() -> (Client, mock::Handle<Request<Body>, Response<Body>>) {
        let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = Client::new(mock_service, "default");
        (client, handle)
    }

    fn mock_pvc(pv_name: Option<&str>) -> PersistentVolumeClaim {
        PersistentVolumeClaim {
            metadata: ObjectMeta {
                name: Some("test-pvc".into()),
                namespace: Some("default".into()),
                ..Default::default()
            },
            spec: Some(PersistentVolumeClaimSpec {
                volume_name: pv_name.map(Into::into),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn mock_pv_not_understood(name: &str) -> PersistentVolume {
        PersistentVolume {
            metadata: ObjectMeta {
                name: Some(name.into()),
                ..Default::default()
            },
            spec: Some(PersistentVolumeSpec {
                // Normally one field would be set here.
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn mock_pv_aws_csi(name: &str, volume_arn: &str) -> PersistentVolume {
        PersistentVolume {
            metadata: ObjectMeta {
                name: Some(name.into()),
                ..Default::default()
            },
            spec: Some(PersistentVolumeSpec {
                csi: Some(CSIPersistentVolumeSource {
                    driver: "ebs.csi.aws.com".into(),
                    volume_handle: volume_arn.into(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn not_bound_returns_none() {
        let (client, _handle) = mock_client();
        let pvc = mock_pvc(None);

        let result = pvc.resolve_cloud_resource(&client).await.unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn pv_found_but_not_understood() {
        let (client, mut handle) = mock_client();
        let pvc = mock_pvc(Some("test-pv"));
        let pv = mock_pv_not_understood("test-pv");

        tokio::spawn(async move {
            let (request, send) = handle.next_request().await.expect("expected a request");

            // Given a bound claim, there must be a request to get the volume.
            assert!(request.uri().path().contains("persistentvolumes"));

            // Send a response back to the controller.
            let body = serde_json::to_vec(&pv).unwrap();
            let response = Response::builder()
                .status(StatusCode::OK)
                .body(Body::from(body))
                .unwrap();
            send.send_response(response);
        });

        let result = pvc.resolve_cloud_resource(&client).await.unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn bound_returns_resource() {
        let (client, mut handle) = mock_client();
        let pvc = mock_pvc(Some("test-pv"));
        let pv = mock_pv_aws_csi(
            "test-pv",
            "arn:aws:ebs:us-east-1:123456789012:volume/vol-0123456789abcdef0",
        );

        tokio::spawn(async move {
            let (request, send) = handle.next_request().await.expect("expected a request");

            // Given a bound claim, there must be a request to get the volume.
            assert!(request.uri().path().contains("persistentvolumes"));

            // Send a response back to the controller.
            let body = serde_json::to_vec(&pv).unwrap();
            let response = Response::builder()
                .status(StatusCode::OK)
                .body(Body::from(body))
                .unwrap();
            send.send_response(response);
        });

        let result = pvc.resolve_cloud_resource(&client).await.unwrap();

        let cr = result.expect("expected CloudResource");
        assert_eq!(
            cr.resource_id,
            "arn:aws:ebs:us-east-1:123456789012:volume/vol-0123456789abcdef0"
        );
    }

    #[tokio::test]
    async fn pv_not_found_returns_error() {
        let (client, mut handle) = mock_client();
        let pvc = mock_pvc(Some("test-pv"));

        tokio::spawn(async move {
            let (_, send) = handle.next_request().await.expect("expected a request");

            // Send a response back to the controller.
            let response = Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from(
                    r#"{"kind":"Status","code":404}"#.as_bytes().to_vec(),
                ))
                .unwrap();
            send.send_response(response);
        });

        let result = pvc.resolve_cloud_resource(&client).await;

        assert!(result.is_err());
    }
}
