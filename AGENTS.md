# Contributor Context

This file provides a quick orientation for contributors (and AI coding agents) working on `k8s-cloud-tagger`.

## What this project does

`k8s-cloud-tagger` is a Kubernetes operator written in Rust. It watches Kubernetes resources and automatically propagates their labels as tags to the corresponding cloud provider resources.

Currently supported:

- **Resources**: `PersistentVolumeClaim` → backing cloud disk
- **Cloud providers**: GCP, AWS and Azure

## Architecture

```
PVC created/updated
        |
   reconcile()          src/reconciler.rs
        |
   resolve_cloud_resource()   src/resources/pvc.rs
        |
   Look up bound PersistentVolume via k8s API
   Extract resource_id from CSI volumeHandle / legacy gcePersistentDisk / hostPath
        |
   If unbound → requeue after requeue_not_ready (default 30s)
        |
   cloud.set_tags(resource_id, labels)   src/cloud/gcp.rs
        |
   GET disk labels+fingerprint → merge k8s labels on top → POST setLabels
        |
   Publish k8s Event (Normal/Tagged) on the PVC
        |
   Requeue after requeue_success (default 5m)
```

## Design constraints for production

This application must install in one command. Typical users manage over 1,000 kubernetes clusters.

No human is going to run a `kubectl` command.

We recommend later versions of `helm`, `kubernetes`, Azure Service Operator, Google Config Connector,
and Amazon Controllers for Kubernetes. If features are ambiguous, we target the latest stable release.

We expect users may run `helm template`, patch with `kustomize`, and then deploy with ArgoCD.

Solve complicated problems in the Rust code. Try to keep Helm simple, deployments need to be deterministic.

## Module map

| Path | Role |
|---|---|
| `src/main.rs` | Entry point: loads config, creates k8s client + cloud client, starts controller loop and health server |
| `src/reconciler.rs` | Core reconciliation logic and error policy; instruments Prometheus metrics |
| `src/traits.rs` | Key abstractions: `CloudTaggable`, `CloudClient` (via `cloud/mod.rs`), `CloudResource`, `CloudProvider` |
| `src/resources/pvc.rs` | `CloudTaggable` impl for `PersistentVolumeClaim`; resolves PV and extracts cloud resource_id |
| `src/cloud/mod.rs` | `CloudClient` trait, `MeteredClient` decorator, `create_client()` factory |
| `src/cloud/gcp.rs` | GCP Compute API: parses CSI handle, sanitises labels, GET+POST disk labels |
| `src/cloud/aws.rs` | AWS EC2 API: parses CSI handle, sanitises tags, STS assume role + EC2 CreateTags |
| `src/cloud/mock.rs` | Mock cloud client used in tests and `cloudProvider: mock` mode |
| `src/config.rs` | Loads runtime config from YAML (`/etc/k8s-cloud-tagger/config.yaml`) |
| `src/metrics.rs` | Prometheus metric definitions |
| `src/health.rs` | Axum HTTP server: `/healthz`, `/readyz`, `/metrics` |
| `src/tls.rs` | rustls setup: ring crypto provider, system CA certs + Mozilla WebPKI roots |
| `src/error.rs` | `thiserror`-derived `Error` enum |
| `helm/k8s-cloud-tagger/` | Helm chart for deploying to Kubernetes |
| `tests/e2e.sh` | End-to-end integration test script (Kind cluster) |
| `xtask/` | Release automation (`cargo xtask release <version>`) |

## Key abstractions

These are the primary extension points:

### `CloudTaggable` — `src/traits.rs`

Implemented by any Kubernetes resource type that can resolve to a backing cloud resource.

```rust
pub trait CloudTaggable: Resource<DynamicType = ()> + Clone + Send + Sync + 'static {
    fn resolve_cloud_resource(
        &self,
        client: &Client,
    ) -> impl Future<Output = Result<Option<CloudResource>, Error>> + Send;
}
```

Returns `None` when the resource is not yet ready (e.g. unbound PVC), triggering a requeue.

### `CloudClient` — `src/cloud/mod.rs`

Implemented by each cloud provider.

```rust
pub trait CloudClient: Send + Sync {
    fn provider_name(&self) -> &'static str;
    async fn set_tags(&self, resource_id: &str, labels: &Labels) -> Result<(), Error>;
}
```

### `CloudResource` — `src/traits.rs`

The data passed from a `CloudTaggable` resolver to a `CloudClient`:

```rust
pub struct CloudResource {
    pub provider: CloudProvider,
    pub resource_id: String,           // e.g. "projects/p/zones/z/disks/d" for GCP
    pub labels: BTreeMap<String, String>,
}
```

## Extending the project

- **New cloud provider**: implement `CloudClient` following `src/cloud/gcp.rs` or `src/cloud/aws.rs` as the reference; add a variant to `CloudProvider` in `src/traits.rs`; wire it into `create_client()` in `src/cloud/mod.rs`.
- **New Kubernetes resource type**: implement `CloudTaggable` following `src/resources/pvc.rs` as the reference; add the controller to `src/main.rs`.

## Development setup

Install [Nix](https://nixos.org/download/) with flakes enabled, then:

```sh
nix develop          # enter dev shell with Rust toolchain, kubectl, helm, kind, etc.
cargo test           # run unit tests
nix flake check      # fmt + clippy + tests
nix run .#kind-test  # full e2e integration test (Kind cluster)
```

The dev shell is the recommended environment — it provides all required tools and the correct Rust toolchain version.

## Helm chart and deployment

The Helm chart lives at `helm/k8s-cloud-tagger/`. Key values:

- `cloudProvider`: `mock` (default), `gcp`, or `aws`
- `requeue.success` / `requeue.notReady` / `requeue.error`: requeue intervals
- `serviceMonitor.enabled`: enables Prometheus Operator `ServiceMonitor`
- `gcp.configConnector.enabled`: optional Config Connector resources for GKE Workload Identity
- `aws.controllersKubernetes.enabled`: optional ACK (AWS Controllers for Kubernetes) resources for EKS IRSA

See `docs/google_cloud.md` for a full GCP/GKE deployment guide and `docs/aws.md` for AWS/EKS deployment.

## Release process

1. Update `CHANGELOG.md` with the new version section.
2. Run `cargo xtask release <version>` — bumps `Cargo.toml` and `helm/k8s-cloud-tagger/Chart.yaml`.
3. Open a PR; merge to `main`.
4. CI detects the version bump, creates a git tag, builds the static musl binary, and pushes the OCI image to `quay.io/upgrades/k8s-cloud-tagger`.
