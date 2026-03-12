# Azure

## Workload Identity

The controller authenticates to Azure using
[AKS Workload Identity](https://learn.microsoft.com/en-us/azure/aks/workload-identity-overview).

The chart sets the `azure.workload.identity/use: "true"` label on the ServiceAccount.
The AKS mutating admission webhook watches for this label and, at pod creation time,
automatically injects the `azure.workload.identity/client-id` annotation and a projected
service account token volume into the pod. No manual credential wiring is required.

**Prerequisites:** the AKS cluster must have both the OIDC issuer and Workload Identity enabled:

```bash
az aks update \
  --resource-group <resourceGroup> \
  --name <clusterName> \
  --enable-oidc-issuer \
  --enable-workload-identity
```

### Azure Service Operator

The Helm chart has optional support for
[Azure Service Operator](https://azure.github.io/azure-service-operator/) (ASO).
When enabled, ASO creates the required Azure resources for you:

- `UserAssignedIdentity` — the managed identity the controller runs as
- `FederatedIdentityCredential` — binds the managed identity to the Kubernetes ServiceAccount
- `RoleAssignment` — grants the built-in
  [Tag Contributor](https://learn.microsoft.com/en-us/azure/role-based-access-control/built-in-roles/management-and-governance#tag-contributor)
  role at subscription scope, allowing the controller to manage tags on any resource
  without broader access

Once ASO has reconciled the identity, the AKS Workload Identity webhook injects the
credentials into the controller pod at runtime.

```bash
export RESOURCE_GROUP=
export LOCATION=
export SUBSCRIPTION_ID=
export OIDC_ISSUER_URL=$(az aks show \
  --resource-group $RESOURCE_GROUP \
  --name <clusterName> \
  --query oidcIssuerProfile.issuerUrl \
  --output tsv)

helm install k8s-cloud-tagger helm/k8s-cloud-tagger \
  --namespace k8s-cloud-tagger \
  --create-namespace \
  --set cloudProvider=azure \
  --set azure.serviceOperator.enabled=true \
  --set azure.serviceOperator.resourceGroup="$RESOURCE_GROUP" \
  --set azure.serviceOperator.location="$LOCATION" \
  --set azure.serviceOperator.subscriptionId="$SUBSCRIPTION_ID" \
  --set azure.serviceOperator.oidcIssuerUrl="$OIDC_ISSUER_URL"
```

## Build an AKS cluster for testing

```bash
export RESOURCE_GROUP=
export LOCATION=
export CLUSTER_NAME=
export SUBSCRIPTION_ID=
export TAG=

# 1. Create a resource group and minimal cluster with OIDC issuer and Workload Identity
az group create \
  --name $RESOURCE_GROUP \
  --location $LOCATION

az aks create \
  --resource-group $RESOURCE_GROUP \
  --name $CLUSTER_NAME \
  --location $LOCATION \
  --node-count 1 \
  --node-vm-size Standard_D2s_v3 \
  --enable-oidc-issuer \
  --enable-workload-identity \
  --generate-ssh-keys

# 2. Create a service principal for Azure Service Operator
ASO_SP=$(az ad sp create-for-rbac \
  --name "aso-${CLUSTER_NAME}" \
  --role Contributor \
  --scopes "/subscriptions/${SUBSCRIPTION_ID}" \
  --output json)

export ASO_CLIENT_ID=$(echo $ASO_SP | jq -r .appId)
export ASO_CLIENT_SECRET=$(echo $ASO_SP | jq -r .password)
export TENANT_ID=$(echo $ASO_SP | jq -r .tenant)

# 3. Connect to the cluster
az aks get-credentials \
  --resource-group $RESOURCE_GROUP \
  --name $CLUSTER_NAME

kubectl create namespace k8s-cloud-tagger

# 4. Install cert-manager (required by ASO)
kubectl apply -f https://github.com/cert-manager/cert-manager/releases/latest/download/cert-manager.yaml

kubectl wait --namespace cert-manager \
  --for=condition=Ready pod --all --timeout=300s

# 5. Install Azure Service Operator (scoped to the CRD groups this project needs)
helm repo add aso2 https://raw.githubusercontent.com/Azure/azure-service-operator/main/v2/charts

helm upgrade --install aso2 aso2/azure-service-operator \
  --create-namespace \
  --namespace azureserviceoperator-system \
  --set crdPattern='resources.azure.com/*;managedidentity.azure.com/*;authorization.azure.com/*'

# 6. Create the ASO credential secret
cat <<EOF | kubectl apply -f -
apiVersion: v1
kind: Secret
metadata:
  name: aso-credential
  namespace: azureserviceoperator-system
stringData:
  AZURE_SUBSCRIPTION_ID: "${SUBSCRIPTION_ID}"
  AZURE_TENANT_ID: "${TENANT_ID}"
  AZURE_CLIENT_ID: "${ASO_CLIENT_ID}"
  AZURE_CLIENT_SECRET: "${ASO_CLIENT_SECRET}"
EOF

# 7. Wait for ASO to be ready
kubectl wait --namespace azureserviceoperator-system \
  --for=condition=Ready pod --all --timeout=300s
```

### Deploy with helm

AKS nodes have unrestricted outbound internet access by default, so images are pulled directly
from `quay.io` — no private registry setup is required.

```bash
export OIDC_ISSUER_URL=$(az aks show \
  --resource-group $RESOURCE_GROUP \
  --name $CLUSTER_NAME \
  --query oidcIssuerProfile.issuerUrl \
  --output tsv)

helm install k8s-cloud-tagger helm/k8s-cloud-tagger \
  --namespace k8s-cloud-tagger \
  --set deployment.env.RUST_BACKTRACE=1 \
  --set deployment.env.RUST_LOG="debug" \
  --set cloudProvider=azure \
  --set azure.serviceOperator.enabled=true \
  --set azure.serviceOperator.resourceGroup="$RESOURCE_GROUP" \
  --set azure.serviceOperator.location="$LOCATION" \
  --set azure.serviceOperator.subscriptionId="$SUBSCRIPTION_ID" \
  --set azure.serviceOperator.oidcIssuerUrl="$OIDC_ISSUER_URL" \
  --set image.tag="${TAG}"
```

### Useful commands

Scale down the cluster to zero (stop paying for compute):

```bash
az aks scale \
  --resource-group $RESOURCE_GROUP \
  --name $CLUSTER_NAME \
  --node-count 0
```

Scale up again:

```bash
az aks scale \
  --resource-group $RESOURCE_GROUP \
  --name $CLUSTER_NAME \
  --node-count 1
```
