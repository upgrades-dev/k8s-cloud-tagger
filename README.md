# K8s-Cloud-Tagger

Kubernetes cloud tagger watches cluster resources and applies labels in your cloud provider.

## Develop

`nix develop` gives you a shell with all the dependencies.

* [Nix](https://nix.dev/install-nix.html)
* [Rust](https://rust-lang.org/tools/install)
* [Docker Desktop](https://docs.docker.com/desktop/use-desktop/)

### Configure nix

You need to enable two experimental features for nix to work.

```bash
mkdir -p ~/.config/nix/
echo "extra-experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf
```

## Test

### Unit tests

```bash
cargo test
```

Run all CI checks locally:

```bash
nix build
```

### Integration tests

**Note to Mac users**: The e2e runs on Mac, but the Dockerimage is built by Docker instead of Nix, due to compatibility issues.

Linux users can opt in to use Docker build with `USE_DOCKER_BUILD=true`, but that is mainly for troubleshooting since it's generally slower than Nix.

```bash
nix develop
KEEP_CLUSTER=true nix run .#kind-test
```

* builds an image using Nix
* creates a Kind cluster
* deploys your image using Helm
* runs the app in test mode
* creates a PVC and listens for an Event

You can also specify the image:
```bash
nix develop
IMAGE=quay.io/upgrades/k8s-cloud-tagger-dev:sha-6f4cbfe nix run .#kind-test
```

`KEEP_CLUSTER=true` prints a message saying how to use `kubectl` in case you want to inspect the cluster.
Otherwise, the cluster is deleted after the test.

To get the raw Kubernetes manifests:

```bash
nix develop
helm template k8s-cloud-tagger helm/k8s-cloud-tagger/ --set serviceMonitor.enabled=true
```

## Deploy

### GKE

Create a low cost, minimal cluster for development:

```bash
gcloud container clusters create cluster-1 \
    --project "${GCP_PROJECT}" \
    --zone "${GCP_ZONE}" \
    --machine-type "e2-small" \
    --disk-type "pd-standard" \
    --disk-size "30" \
    --spot \
    --num-nodes 1 \
    --logging=NONE \
    --monitoring=NONE \
    --no-enable-managed-prometheus \
    --release-channel "stable" \
    --addons GcePersistentDiskCsiDriver
```

Load the cluster's kube config:

```bash
gcloud container clusters get-credentials cluster-1 \
  --zone "${GCP_ZONE}" \
  --project "${GCP_PROJECT}"
```

#### IAM setup

Grant IAM permissions to the controller's service account:

```bash
# Set your project ID once
export PROJECT_ID="<your-gcp-project-id>"
gcloud config set project "$PROJECT_ID"

# Create a service account
gcloud iam service-accounts create k8s-cloud-tagger \
  --display-name="k8s-cloud-tagger"

# Grant permissions (scope down for production use)
gcloud projects add-iam-policy-binding "$PROJECT_ID" \
  --member="serviceAccount:k8s-cloud-tagger@${PROJECT_ID}.iam.gserviceaccount.com" \
  --role="roles/compute.storageAdmin"

# Bind the GCP and K8s service accounts
gcloud iam service-accounts add-iam-policy-binding \
  "k8s-cloud-tagger@${PROJECT_ID}.iam.gserviceaccount.com" \
  --role="roles/iam.workloadIdentityUser" \
  --member="serviceAccount:${PROJECT_ID}.svc.id.goog[k8s-cloud-tagger/k8s-cloud-tagger]"

# Add the GCP service account annotation to the controller's service account
kubectl annotate serviceaccount k8s-cloud-tagger \
  -n k8s-cloud-tagger \
  "iam.gke.io/gcp-service-account=k8s-cloud-tagger@${PROJECT_ID}.iam.gserviceaccount.com" \
  --overwrite
```

#### Helm install

You can let GitHub CI build and push your branch's image and push it to Quay, and have the cluster pull the image from there. If you use an autopilot cluster or just have private nodes, then it's easiest to use Google Artifact Registry.

##### Push image to Quay

Build and push an image from your branch with the [push-dev-image](https://github.com/upgrades-dev/k8s-cloud-tagger/actions/workflows/push-dev-image.yml) GHA job.

```bash
helm install k8s-cloud-tagger helm/k8s-cloud-tagger \
  --set deployment.env.RUST_LOG="debug" \
  --set cloudProvider=gcp \
  --set image.repository=quay.io/upgrades/k8s-cloud-tagger-dev \
  --set image.tag="sha-$(git rev-parse --short HEAD)"
```

Where the value for `image.tag` matches the tag of the image pushed to [Quay](https://quay.io/repository/upgrades/k8s-cloud-tagger-dev?tab=tags).

##### Push image to Google Artifact Registry

```bash
nix develop
nix build .#image-dev

docker load < result
docker tag quay.io/upgrades/k8s-cloud-tagger-dev:dev \
  "${REGION}-docker.pkg.dev/${PROJECT_ID}/k8s-cloud-tagger/controller:YOUR-FEATURE"

helm upgrade k8s-cloud-tagger helm/k8s-cloud-tagger -n k8s-cloud-tagger \
  --set deployment.env.RUST_LOG="debug" --set cloudProvider=gcp \
  --set image.repository="${REGION}-docker.pkg.dev/${PROJECT_ID}/k8s-cloud-tagger/controller" \
  --set image.tag="YOUR-FEATURE"
```

#### Useful commands

Scale down the cluster to zero (stop paying for compute):

```bash
gcloud container clusters resize cluster-1 \
    --node-pool default-pool \
    --num-nodes 0 \
    --zone "${GCP_ZONE}" \
    --project "${GCP_PROJECT}"
```

Scale up again:

```bash
gcloud container clusters resize cluster-1 \
    --node-pool default-pool \
    --num-nodes 1 \
    --zone "${GCP_ZONE}" \
    --project "${GCP_PROJECT}"
```

#### Label sanitisation

Kubernetes label keys and values can contain characters that are not valid in GCP labels.
GCP labels only allow lowercase letters, digits, hyphens, and underscores (`[a-z0-9_-]`),
with keys limited to 63 characters and required to start with a lowercase letter.
To bridge this gap, k8s-cloud-tagger sanitises labels before applying them to cloud resources:
all characters are lowercased, and any character outside the allowed set is replaced with a hyphen.
This follows the conventions used by Google's own resource labels (such as the `goog-gke-*` labels applied by GKE),
where hyphens are the standard word separator.
For more detail on GCP label requirements, see the [Google Cloud labeling best practices](https://cloud.google.com/resource-manager/docs/best-practices-labels).

| Kubernetes label | GCP label |
| --- | --- |
| `app.kubernetes.io/name: frontend` | `app-kubernetes-io-name: frontend` |
| `helm.sh/chart: myapp-1.2.0` | `helm-sh-chart: myapp-1-2-0` |
| `env: production` | `env: production` |
| `upgrades.dev/managed-by: k8s-cloud-tagger` | `upgrades-dev-managed-by: k8s-cloud-tagger` |
| `Team: Platform` | `team: platform` |

## Release

1. Check out a new branch
1. Update [CHANGELOG.md](./CHANGELOG.md) following the [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format and [SemVer](https://semver.org/) conventions.
1. Run `cargo xtask release <version>`. This updates and commits the relevant files to git.
1. Raise PR, merge PR
1. When the PR is merged, a job will run that adds a git tag, and builds and pushes the release Docker image