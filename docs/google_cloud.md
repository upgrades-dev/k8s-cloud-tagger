# Build a GKE cluster for testing

```bash
export PROJECT_ID=
export CLUSTER_NAME=
export REGION=
export ZONE=

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


# 3. Connect your new cluster
gcloud container clusters get-credentials $CLUSTER_NAME --zone=$ZONE --project=$PROJECT_ID

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

## Deploy with helm

```bash
helm install k8s-cloud-tagger helm/k8s-cloud-tagger -n k8s-cloud-tagger \
  --create-namespace \
  --set deployment.env.RUST_BACKTRACE=1 \
  --set deployment.env.RUST_LOG="debug"\
  --set cloudProvider=gcp \
  --set gcp.projectId="${PROJECT_ID}" \
  --set gcp.configConnector.enabled=true \
  --set image.repository="${REGION}-docker.pkg.dev/${PROJECT_ID}/k8s-cloud-tagger/controller" \
  --set image.tag="YOUR-FEATURE" \
  --set gcp.configConnector.serviceAccount="k8s-cloud-tagger-test" \
  --set gcp.configConnector.customRoleName="k8s_cloud_tagger_test"
```
