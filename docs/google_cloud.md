# Google Cloud Platform

## Workload Identity

The controller requires a GCP service account with `compute.disks.get` and `compute.disks.setLabels`.
Create a role and bind to the Kubernetes service account via Workload Identity.

You can do this with the `gcloud` and `kubectl` commands:

```bash
export K8S_NAMESPACE="k8s-cloud-tagger"
export GCP_PROJECT_ID="your-project-id"

# Create a custom role with minimal permissions
gcloud iam roles create k8sCloudTaggerRole \
  --project="$GCP_PROJECT_ID" \
  --title="k8s-cloud-tagger Disk Labeler" \
  --description="Read and set labels on Compute Engine disks" \
  --permissions="compute.disks.get,compute.disks.setLabels"

# Create a google service account
gcloud iam service-accounts create k8s-cloud-tagger \
  --display-name="k8s-cloud-tagger"

# Grant the custom role (minimal permissions)
gcloud projects add-iam-policy-binding "$GCP_PROJECT_ID" \
  --member="serviceAccount:k8s-cloud-tagger@${GCP_PROJECT_ID}.iam.gserviceaccount.com" \
  --role="projects/${GCP_PROJECT_ID}/roles/k8sCloudTaggerRole"

# Bind the GCP and K8s service accounts
gcloud iam service-accounts add-iam-policy-binding \
  "k8s-cloud-tagger@${GCP_PROJECT_ID}.iam.gserviceaccount.com" \
  --role="roles/iam.workloadIdentityUser" \
  --member="serviceAccount:${GCP_PROJECT_ID}.svc.id.goog[${K8S_NAMESPACE}/k8s-cloud-tagger]"

# Add the GCP service account annotation to the controller's service account
kubectl annotate serviceaccount k8s-cloud-tagger \
  "iam.gke.io/gcp-service-account=k8s-cloud-tagger@${GCP_PROJECT_ID}.iam.gserviceaccount.com" \
  --overwrite \
  --namespace="$K8S_NAMESPACE"
```

### Google Config Connector

Our Helm chart has optional support for [Google Config Connector](https://cloud.google.com/config-connector/docs/overview).

In the guide above, where we create a role and bind service accounts.
This can all be done for you with Config Connector.

```
--set gcp.configConnector.enabled=true
```

## Build a GKE cluster for testing

```bash
export PROJECT_ID=
export CLUSTER_NAME=
export REGION=
export ZONE=
export TAG=

# 1. Create a minimal Standard cluster with Config Connector
gcloud container clusters create $CLUSTER_NAME \
  --zone=$ZONE \
  --project=$PROJECT_ID \
  --num-nodes=1 \
  --machine-type=e2-standard-4 \
  --addons=ConfigConnector \
  --workload-pool=${PROJECT_ID}.svc.id.goog \
  --no-enable-master-authorized-networks \
  --release-channel=rapid

gcloud services enable cloudresourcemanager.googleapis.com --project=$PROJECT_ID

gcloud services enable iam.googleapis.com --project=$PROJECT_ID

# 2. Service account
gcloud iam service-accounts create config-connector-sa --project=$PROJECT_ID

gcloud projects add-iam-policy-binding $PROJECT_ID \
  --member="serviceAccount:config-connector-sa@${PROJECT_ID}.iam.gserviceaccount.com" \
  --role="roles/editor"

gcloud iam service-accounts add-iam-policy-binding \
  config-connector-sa@${PROJECT_ID}.iam.gserviceaccount.com \
  --member="serviceAccount:${PROJECT_ID}.svc.id.goog[cnrm-system/cnrm-controller-manager]" \
  --role="roles/iam.workloadIdentityUser"

gcloud iam service-accounts add-iam-policy-binding \
  config-connector-sa@${PROJECT_ID}.iam.gserviceaccount.com \
  --member="serviceAccount:${PROJECT_ID}.svc.id.goog[cnrm-system/cnrm-controller-manager-k8s-cloud-tagger]" \
  --role="roles/iam.workloadIdentityUser"
  
gcloud projects add-iam-policy-binding $PROJECT_ID \
  --member="serviceAccount:config-connector-sa@${PROJECT_ID}.iam.gserviceaccount.com" \
  --role="roles/iam.serviceAccountAdmin"

gcloud projects add-iam-policy-binding $PROJECT_ID \
  --member="serviceAccount:config-connector-sa@${PROJECT_ID}.iam.gserviceaccount.com" \
  --role="roles/iam.roleAdmin"

gcloud projects add-iam-policy-binding $PROJECT_ID \
  --member="serviceAccount:config-connector-sa@${PROJECT_ID}.iam.gserviceaccount.com" \
  --role="roles/resourcemanager.projectIamAdmin"

# 3. Connect your new cluster and prepare a namespace
gcloud container clusters get-credentials $CLUSTER_NAME --zone=$ZONE --project=$PROJECT_ID

kubectl create namespace k8s-cloud-tagger

# 4. Configure Config Connector
cat <<EOF | kubectl apply -f -
apiVersion: core.cnrm.cloud.google.com/v1beta1
kind: ConfigConnectorContext
metadata:
  name: configconnectorcontext.core.cnrm.cloud.google.com
  namespace: k8s-cloud-tagger
spec:
  googleServiceAccount: "config-connector-sa@${PROJECT_ID}.iam.gserviceaccount.com"
EOF

kubectl annotate namespace default cnrm.cloud.google.com/project-id=$PROJECT_ID

# 5. Wait for it
kubectl wait -n cnrm-system --for=condition=Ready pod --all --timeout=300s
```

### Push to Artifact Registry

If you use private nodes, like in this test, then it's easiest to push images to
[Artifact Registry](https://docs.cloud.google.com/artifact-registry/docs).

```bash
nix develop
nix build .#image-dev
docker load < result
docker tag quay.io/upgrades/k8s-cloud-tagger-dev:dev \
  "${REGION}-docker.pkg.dev/${PROJECT}/k8s-cloud-tagger/controller:${TAG}"
docker push "${REGION}-docker.pkg.dev/${PROJECT}/k8s-cloud-tagger/controller:${TAG}"
```

### Deploy with helm

```bash
helm install k8s-cloud-tagger helm/k8s-cloud-tagger -n k8s-cloud-tagger \
  --set deployment.env.RUST_BACKTRACE=1 \
  --set deployment.env.RUST_LOG="debug"\
  --set cloudProvider=gcp \
  --set gcp.projectId="${PROJECT_ID}" \
  --set gcp.configConnector.enabled=true \
  --set image.repository="${REGION}-docker.pkg.dev/${PROJECT_ID}/k8s-cloud-tagger/controller" \
  --set image.tag="${TAG}" \
  --set gcp.configConnector.serviceAccount="k8s-cloud-tagger-test" \
  --set gcp.configConnector.customRoleName="k8s_cloud_tagger_test"
```

### Useful commands

Scale down the cluster to zero (stop paying for compute):

```bash
gcloud container clusters resize cluster-1 \
    --node-pool default-pool \
    --num-nodes 0 \
    --zone "${GCP_ZONE}" \
    --project "${GCP_PROJECT_ID}"
```

Scale up again:

```bash
gcloud container clusters resize cluster-1 \
    --node-pool default-pool \
    --num-nodes 1 \
    --zone "${GCP_ZONE}" \
    --project "${GCP_PROJECT_ID}"
```
