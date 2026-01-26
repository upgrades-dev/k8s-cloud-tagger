use crate::traits::CloudTaggable;
use http::{Request, Response, StatusCode};
use k8s_openapi::api::core::v1::{
    CSIPersistentVolumeSource, PersistentVolume, PersistentVolumeClaim, PersistentVolumeClaimSpec,
    PersistentVolumeSpec,
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
