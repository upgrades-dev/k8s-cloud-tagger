use crate::error::Error;
use crate::traits::{CloudProvider, CloudResource, CloudTaggable};
use k8s_openapi::api::core::v1::{PersistentVolume, PersistentVolumeClaim};
use kube::{Api, Client};

fn provider_from_csi_driver(driver: &str) -> CloudProvider {
    match driver {
        "ebs.csi.aws.com" => CloudProvider::Aws,
        "disk.csi.azure.com" => CloudProvider::Azure,
        "pd.csi.storage.gke.io" => CloudProvider::Gcp,
        _ => CloudProvider::Other,
    }
}

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

            let Some((provider, resource_id)) = extract_resource_id(&pv) else {
                tracing::debug!(pv = %pv_name, "No supported volume source found");
                return Ok(None);
            };

            tracing::debug!(%resource_id, "Found volume");

            Ok(Some(CloudResource {
                provider,
                resource_id,
                labels,
            }))
        }
    }
}

fn extract_resource_id(pv: &PersistentVolume) -> Option<(CloudProvider, String)> {
    let spec = pv.spec.as_ref()?;

    // CSI is the most common and modern.
    if let Some(csi) = &spec.csi {
        let provider = provider_from_csi_driver(&csi.driver);
        return Some((provider, csi.volume_handle.clone()));
    }

    // Google Compute Engine Persistent Disk (found on older GKE clusters)
    if let Some(pd_name) = &spec.gce_persistent_disk {
        return Some((CloudProvider::Gcp, pd_name.pd_name.clone()));
    }

    // hostPath - used by Kind/local-path-provisioner for test environments
    if let Some(host_path) = &spec.host_path {
        return Some((CloudProvider::Other, host_path.path.clone()));
    }

    let pv_name = pv.metadata.name.as_deref().unwrap_or("<unknown>");
    tracing::warn!(pv = %pv_name, "No supported volume source found (expected CSI, GCE PD, or hostPath)");

    None
}

#[cfg(test)]
mod tests {
    use crate::traits::{CloudProvider, CloudTaggable};
    use http::{Request, Response, StatusCode};
    use k8s_openapi::api::core::v1::{
        CSIPersistentVolumeSource, HostPathVolumeSource, PersistentVolume, PersistentVolumeClaim,
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

    fn respond_with_pv(
        mut handle: mock::Handle<Request<Body>, Response<Body>>,
        pv: PersistentVolume,
    ) {
        tokio::spawn(async move {
            let (request, send) = handle.next_request().await.expect("expected a request");
            assert!(request.uri().path().contains("persistentvolumes"));
            let body = serde_json::to_vec(&pv).unwrap();
            let response = Response::builder()
                .status(StatusCode::OK)
                .body(Body::from(body))
                .unwrap();
            send.send_response(response);
        });
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

    fn mock_pv_azure_csi(name: &str, volume_id: &str) -> PersistentVolume {
        PersistentVolume {
            metadata: ObjectMeta {
                name: Some(name.into()),
                ..Default::default()
            },
            spec: Some(PersistentVolumeSpec {
                csi: Some(CSIPersistentVolumeSource {
                    driver: "disk.csi.azure.com".into(),
                    volume_handle: volume_id.into(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn mock_pv_gcp_csi(name: &str, pd_name: &str) -> PersistentVolume {
        PersistentVolume {
            metadata: ObjectMeta {
                name: Some(name.into()),
                ..Default::default()
            },
            spec: Some(PersistentVolumeSpec {
                csi: Some(CSIPersistentVolumeSource {
                    driver: "pd.csi.storage.gke.io".into(),
                    volume_handle: pd_name.into(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn mock_pv_local(name: &str, path: &str) -> PersistentVolume {
        PersistentVolume {
            metadata: ObjectMeta {
                name: Some(name.into()),
                ..Default::default()
            },
            spec: Some(PersistentVolumeSpec {
                host_path: Some(HostPathVolumeSource {
                    path: path.into(),
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
        let (client, handle) = mock_client();
        let pvc = mock_pvc(Some("test-pv"));
        let pv = mock_pv_not_understood("test-pv");

        respond_with_pv(handle, pv);

        let result = pvc.resolve_cloud_resource(&client).await.unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn bound_returns_aws_resource() {
        let (client, handle) = mock_client();
        let pvc = mock_pvc(Some("test-pv"));
        let pv = mock_pv_aws_csi(
            "test-pv",
            "arn:aws:ebs:us-east-1:123456789012:volume/vol-0123456789cafe0",
        );

        respond_with_pv(handle, pv);

        let result = pvc.resolve_cloud_resource(&client).await.unwrap();

        let cr = result.expect("expected CloudResource");
        assert_eq!(
            cr.resource_id,
            "arn:aws:ebs:us-east-1:123456789012:volume/vol-0123456789cafe0"
        );
        assert_eq!(cr.provider, CloudProvider::Aws);
    }

    #[tokio::test]
    async fn bound_returns_azure_resource() {
        let (client, handle) = mock_client();
        let pvc = mock_pvc(Some("test-pv"));
        let pv = mock_pv_azure_csi(
            "test-pv",
            "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.Compute/disks/test-disk",
        );

        respond_with_pv(handle, pv);

        let result = pvc.resolve_cloud_resource(&client).await.unwrap();

        let cr = result.expect("expected CloudResource");
        assert_eq!(
            cr.resource_id,
            "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.Compute/disks/test-disk"
        );
        assert_eq!(cr.provider, CloudProvider::Azure);
    }

    #[tokio::test]
    async fn bound_returns_gcp_resource() {
        let (client, handle) = mock_client();
        let pvc = mock_pvc(Some("test-pv"));
        let pv = mock_pv_gcp_csi(
            "test-pv",
            "projects/test-project-123456/zones/us-central1-a/disks/pvc-a1b2c3d4-e5f6-7890-cafe-ef1234567890",
        );

        respond_with_pv(handle, pv);

        let result = pvc.resolve_cloud_resource(&client).await.unwrap();

        let cr = result.expect("expected CloudResource");
        assert_eq!(
            cr.resource_id,
            "projects/test-project-123456/zones/us-central1-a/disks/pvc-a1b2c3d4-e5f6-7890-cafe-ef1234567890"
        );
        assert_eq!(cr.provider, CloudProvider::Gcp);
    }

    #[tokio::test]
    async fn bound_returns_other_for_local_path() {
        let (client, handle) = mock_client();
        let pvc = mock_pvc(Some("test-pv"));
        let pv = mock_pv_local("test-pv", "/var/local-path-provisioner/pvc-abc123");

        respond_with_pv(handle, pv);

        let result = pvc.resolve_cloud_resource(&client).await.unwrap();

        let cr = result.expect("expected CloudResource");
        assert_eq!(cr.resource_id, "/var/local-path-provisioner/pvc-abc123");
        assert_eq!(cr.provider, CloudProvider::Other);
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
