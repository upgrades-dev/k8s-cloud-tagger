# Azure (AKS)

This document covers deploying `k8s-cloud-tagger` on AKS using
[Workload Identity](https://learn.microsoft.com/en-us/azure/aks/workload-identity-overview) for
authentication and [Azure Service Operator](https://azure.github.io/azure-service-operator/) (ASO)
to manage the required Azure resources.

## How it works

The chart sets the `azure.workload.identity/use: "true"` label on the pod template and the
`azure.workload.identity/client-id` annotation on the ServiceAccount. At pod creation time the AKS
Workload Identity webhook injects `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_AUTHORITY_HOST`, and
`AZURE_FEDERATED_TOKEN_FILE` into the pod. The controller uses these to obtain an ARM bearer token
and call the Tags API.

When `azure.serviceOperator.enabled=true`, ASO creates and manages:
- A `UserAssignedIdentity` (the managed identity)
- A `FederatedIdentityCredential` (the OIDC trust binding between the identity and the ServiceAccount)
- A `RoleAssignment` granting [Tag Contributor](https://learn.microsoft.com/en-us/azure/role-based-access-control/built-in-roles/management-and-governance#tag-contributor) at subscription scope

All ASO resources are `detach-on-delete` — `helm uninstall` will not delete them in Azure.

> **Note:** The managed identity must be pre-created before `helm install` because its `clientId`
> must be known at install time to annotate the ServiceAccount. ASO will adopt and manage the
> identity going forward.

## 1. Set environment variables

```bash
export RESOURCE_GROUP=
export LOCATION=
export CLUSTER_NAME=
export SUBSCRIPTION_ID=
export TAG=          # image tag, e.g. sha-63d1b9b
```

## 2. Create the AKS cluster

```bash
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

az aks get-credentials \
  --resource-group $RESOURCE_GROUP \
  --name $CLUSTER_NAME
```

## 3. Install cert-manager

Required by ASO.

```bash
kubectl apply -f https://github.com/cert-manager/cert-manager/releases/latest/download/cert-manager.yaml

kubectl wait --namespace cert-manager \
  --for=condition=Ready pod --all --timeout=300s
```

## 4. Install Azure Service Operator

Scoped to only the CRD groups this project needs.

```bash
helm repo add aso2 https://raw.githubusercontent.com/Azure/azure-service-operator/main/v2/charts

helm upgrade --install aso2 aso2/azure-service-operator \
  --create-namespace \
  --namespace azureserviceoperator-system \
  --set crdPattern='resources.azure.com/*;managedidentity.azure.com/*;authorization.azure.com/*'

kubectl wait --namespace azureserviceoperator-system \
  --for=condition=Ready pod --all --timeout=300s
```

## 5. Create a service principal for ASO

ASO uses this credential to manage Azure resources on your behalf.

> **Note:** `Owner` is used here for convenience. Fine-grained permissions should be configured
> before production use.

```bash
ASO_SP=$(az ad sp create-for-rbac \
  --name "aso-${CLUSTER_NAME}" \
  --role Owner \
  --scopes "/subscriptions/${SUBSCRIPTION_ID}" \
  --output json)

export ASO_CLIENT_ID=$(echo $ASO_SP | jq -r .appId)
export ASO_CLIENT_SECRET=$(echo $ASO_SP | jq -r .password)
export TENANT_ID=$(echo $ASO_SP | jq -r .tenant)
```

## 6. Configure ASO credentials

ASO uses per-namespace credentials. Create the secret in the same namespace as the chart resources.

```bash
kubectl create namespace k8s-cloud-tagger

cat <<EOF | kubectl apply -f -
apiVersion: v1
kind: Secret
metadata:
  name: aso-credential
  namespace: k8s-cloud-tagger
stringData:
  AZURE_SUBSCRIPTION_ID: "${SUBSCRIPTION_ID}"
  AZURE_TENANT_ID: "${TENANT_ID}"
  AZURE_CLIENT_ID: "${ASO_CLIENT_ID}"
  AZURE_CLIENT_SECRET: "${ASO_CLIENT_SECRET}"
EOF
```

## 7. Create the managed identity

The `clientId` must be known before `helm install` to annotate the ServiceAccount.

```bash
az identity create \
  --resource-group $RESOURCE_GROUP \
  --name k8s-cloud-tagger \
  --location $LOCATION

export CLIENT_ID=$(az identity show \
  --resource-group $RESOURCE_GROUP \
  --name k8s-cloud-tagger \
  --query clientId \
  --output tsv)

export OIDC_ISSUER_URL=$(az aks show \
  --resource-group $RESOURCE_GROUP \
  --name $CLUSTER_NAME \
  --query oidcIssuerProfile.issuerUrl \
  --output tsv)
```

## 8. Install the Helm chart

AKS nodes have unrestricted outbound internet access by default — images are pulled directly from
`quay.io` without any private registry setup.

```bash
helm install k8s-cloud-tagger helm/k8s-cloud-tagger \
  --namespace k8s-cloud-tagger \
  --create-namespace \
  --set cloudProvider=azure \
  --set azure.clientId="$CLIENT_ID" \
  --set azure.serviceOperator.enabled=true \
  --set azure.serviceOperator.resourceGroup="$RESOURCE_GROUP" \
  --set azure.serviceOperator.location="$LOCATION" \
  --set azure.serviceOperator.subscriptionId="$SUBSCRIPTION_ID" \
  --set azure.serviceOperator.oidcIssuerUrl="$OIDC_ISSUER_URL" \
  --set deployment.env.RUST_BACKTRACE=1 \
  --set deployment.env.RUST_LOG="debug" \
  --set image.repository="quay.io/upgrades/k8s-cloud-tagger-dev" \
  --set image.tag="${TAG}"
```

ASO will reconcile the `FederatedIdentityCredential` and `RoleAssignment` automatically. The
controller pod will start tagging once the role assignment propagates (typically within a minute).

## 9. Verify

Check ASO resources are ready:

```bash
kubectl get userassignedidentity,federatedidentitycredential,roleassignment \
  -n k8s-cloud-tagger
```

Check the controller is tagging:

```bash
kubectl logs -n k8s-cloud-tagger \
  -l app.kubernetes.io/name=k8s-cloud-tagger \
  --tail=20
```

Look for `Azure: tags merged` in the output.

## Cluster management

Scale down to zero (stop paying for compute):

```bash
az aks scale \
  --resource-group $RESOURCE_GROUP \
  --name $CLUSTER_NAME \
  --node-count 0
```

Scale back up:

```bash
az aks scale \
  --resource-group $RESOURCE_GROUP \
  --name $CLUSTER_NAME \
  --node-count 1
```
