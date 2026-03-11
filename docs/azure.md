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
