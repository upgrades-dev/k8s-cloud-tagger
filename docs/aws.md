# Amazon Web Services (EKS)

This document covers deploying `k8s-cloud-tagger` on EKS using
[IAM Roles for Service Accounts](https://docs.aws.amazon.com/eks/latest/userguide/iam-roles-for-service-accounts.html) (IRSA) for
authentication and [AWS Controllers for Kubernetes](https://aws-controllers-k8s.github.io/community/) (ACK) to manage the required IAM resources.

## How it works

The chart sets the `eks.amazonaws.com/role-arn` annotation on the ServiceAccount.
At pod creation time the EKS pod identity webhook injects `AWS_WEB_IDENTITY_TOKEN_FILE`
and `AWS_ROLE_ARN` environment variables into the pod. The controller uses these to
obtain temporary AWS credentials from STS and call the EC2 CreateTags API.

When `aws.controllersKubernetes.enabled=true`, ACK creates and manages:
- An `IAM Role` with an IRSA trust policy
- An inline IAM policy granting `ec2:DescribeVolumes` and `ec2:CreateTags`

AWS performs server-side merge when applying tags, so the controller does not need
to fetch existing tags first. AWS tags are case-sensitive (unlike GCP labels).

> **Note:** The IAM role is created at runtime by ACK. The pod may restart once or twice
> while waiting for the role to be created (typically 10-30 seconds).

## End-to-End Testing (From Scratch)

This guide walks through testing `k8s-cloud-tagger` in a real EKS cluster, starting from scratch. After testing, the cluster is deleted to avoid ongoing charges.

**Prerequisites:**
- `aws` CLI (v2), `eksctl`, `kubectl`, `helm` (v3.8+), and `jq` installed
- AWS credentials configured (`aws configure`)
- Estimated cost: ~$0.25-0.50 USD for a 2-hour test

**Using Nix:**

If you have Nix with flakes enabled, all required tools are available in the AWS dev shell:

```bash
nix develop .#aws
```

**Cost optimization:**
- Using t3.small spot instances (70% cheaper than on-demand)
- Single-node cluster (minimum viable configuration)
- Total cost: ~$0.10/hour (EKS control plane) + ~$0.006/hour (spot instance)

### 1. Set environment variables

```bash
export AWS_REGION=eu-west-2  # or your preferred region
export CLUSTER_NAME=k8s-cloud-tagger-test
export NAMESPACE=k8s-cloud-tagger
export TAG=latest  # or specific SHA like sha-63d1b9b
export ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)

echo "Account ID: $ACCOUNT_ID"
echo "Region: $AWS_REGION"
```

### 2. Create an EKS cluster

Create a minimal EKS cluster with spot instances for cost savings:

```bash
eksctl create cluster \
  --name $CLUSTER_NAME \
  --region $AWS_REGION \
  --node-type t3.small \
  --nodes 1 \
  --nodes-min 1 \
  --nodes-max 1 \
  --spot \
  --with-oidc
```

This takes 15-20 minutes. The `--with-oidc` flag automatically configures IRSA (IAM Roles for Service Accounts).

### 3. Install ACK IAM Controller

Install the ACK IAM controller using Helm:

```bash
export RELEASE_VERSION=$(curl -sL https://api.github.com/repos/aws-controllers-k8s/iam-controller/releases/latest | jq -r '.tag_name | ltrimstr("v")')
export ACK_SYSTEM_NAMESPACE=ack-system

# Log in to ECR Public
aws ecr-public get-login-password --region us-east-1 | \
  helm registry login --username AWS --password-stdin public.ecr.aws

# Install ACK IAM controller
helm install --create-namespace -n $ACK_SYSTEM_NAMESPACE \
  ack-iam-controller \
  oci://public.ecr.aws/aws-controllers-k8s/iam-chart \
  --version=$RELEASE_VERSION \
  --set=aws.region=$AWS_REGION
```

### 4. Configure ACK permissions

Create an IAM role for the ACK controller itself (so it can manage IAM resources):

```bash
eksctl create iamserviceaccount \
  --name ack-iam-controller \
  --namespace $ACK_SYSTEM_NAMESPACE \
  --cluster $CLUSTER_NAME \
  --region $AWS_REGION \
  --role-name ack-iam-controller-${CLUSTER_NAME} \
  --attach-policy-arn arn:aws:iam::aws:policy/IAMFullAccess \
  --approve \
  --override-existing-serviceaccounts

# Restart ACK controller to pick up the IAM role
kubectl rollout restart deployment -n $ACK_SYSTEM_NAMESPACE ack-iam-controller-iam-chart
```

> **Note:** `IAMFullAccess` is used for simplicity in testing. Production deployments should use least-privilege policies.

### 5. Get OIDC issuer URL

```bash
export OIDC_ISSUER_URL=$(aws eks describe-cluster \
  --name $CLUSTER_NAME \
  --region $AWS_REGION \
  --query cluster.identity.oidc.issuer \
  --output text)

echo "OIDC Issuer: $OIDC_ISSUER_URL"
```

### 6. Install k8s-cloud-tagger

```bash
helm install k8s-cloud-tagger helm/k8s-cloud-tagger \
  --namespace $NAMESPACE \
  --create-namespace \
  --set cloudProvider=aws \
  --set aws.controllersKubernetes.enabled=true \
  --set aws.controllersKubernetes.accountId="$ACCOUNT_ID" \
  --set aws.controllersKubernetes.oidcIssuerUrl="$OIDC_ISSUER_URL" \
  --set image.repository=quay.io/upgrades/k8s-cloud-tagger-dev \
  --set image.tag="$TAG" \
  --set deployment.env.RUST_LOG="debug" \
  --set deployment.env.RUST_BACKTRACE=1 \
  --wait \
  --timeout 5m
```

The Helm chart will create an ACK `Role` resource. ACK will then create the actual IAM role in AWS.

### 7. Wait for IAM role creation

ACK typically takes 30-60 seconds to create the IAM role:

```bash
echo "Waiting for ACK to create IAM role (typically 30-60 seconds)..."
sleep 60

# Check ACK resources
kubectl get role.iam.services.k8s.aws -n $NAMESPACE
```

You should see output like:
```
NAME                        AGE
k8s-cloud-tagger-role       1m
```

### 8. Verify controller is running

```bash
kubectl get pods -n $NAMESPACE
kubectl logs -n $NAMESPACE -l app.kubernetes.io/name=k8s-cloud-tagger --tail=50
```

The controller may have restarted once or twice while waiting for the IAM role. Once the role is ready, you should see log messages indicating the controller is running.

### 9. Create test PVC

Create a PVC with labels that should be propagated to the EBS volume:

```bash
cat <<EOF | kubectl apply -f -
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: test-pvc
  namespace: default
  labels:
    environment: test
    team: platform
    cost-center: engineering
spec:
  accessModes:
    - ReadWriteOnce
  resources:
    requests:
      storage: 1Gi
  storageClassName: gp2
EOF

# Wait for PVC to bind
kubectl wait --for=jsonpath='{.status.phase}'=Bound \
  pvc/test-pvc -n default --timeout=60s
```

### 10. Verify tags on EBS volume

Get the volume ID and check its tags in AWS:

```bash
# Get volume ID from the bound PV
PV_NAME=$(kubectl get pvc test-pvc -n default -o jsonpath='{.spec.volumeName}')
VOLUME_ID=$(kubectl get pv $PV_NAME -o jsonpath='{.spec.csi.volumeHandle}' | awk -F/ '{print $NF}')

echo "Volume ID: $VOLUME_ID"

# Wait a moment for tagging to complete
sleep 10

# Check tags on the volume
aws ec2 describe-volumes \
  --region $AWS_REGION \
  --volume-ids $VOLUME_ID \
  --query 'Volumes[0].Tags' \
  --output table
```

You should see tags including `environment=test`, `team=platform`, and `cost-center=engineering`.

### 11. Check Kubernetes events

Check for the `Tagged` event from k8s-cloud-tagger:

```bash
kubectl get events -n default \
  --field-selector involvedObject.name=test-pvc,reason=Tagged
```

Look for a `Normal/Tagged` event that confirms the tagging operation succeeded.

### 12. Cleanup

**IMPORTANT:** Delete the cluster to avoid ongoing charges.

```bash
# Delete test PVC first (removes the EBS volume)
kubectl delete pvc test-pvc -n default

# Uninstall Helm releases
helm uninstall k8s-cloud-tagger -n $NAMESPACE
helm uninstall ack-iam-controller -n $ACK_SYSTEM_NAMESPACE

# Delete the EKS cluster
eksctl delete cluster --name $CLUSTER_NAME --region $AWS_REGION
```

Cluster deletion takes 10-15 minutes. Verify it completes:

```bash
eksctl get cluster --name $CLUSTER_NAME --region $AWS_REGION
```

You should see "No clusters found" when deletion is complete.

## Production Deployment Prerequisites

- EKS cluster with IRSA enabled (OIDC provider configured)
- [ACK IAM controller](https://github.com/aws-controllers-k8s/iam-controller) installed in the cluster
- kubectl configured with cluster access
- Helm 3.x installed
- AWS CLI (optional, for verification)

## 1. Get required values

Export the AWS Account ID and OIDC issuer URL from your EKS cluster:

```bash
export CLUSTER_NAME=<your-cluster-name>
export REGION=<your-region>

export ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)

export OIDC_ISSUER_URL=$(aws eks describe-cluster \
  --name $CLUSTER_NAME \
  --region $REGION \
  --query cluster.identity.oidc.issuer \
  --output text)

echo "Account ID: $ACCOUNT_ID"
echo "OIDC Issuer: $OIDC_ISSUER_URL"
```

## 2. Create the namespace

```bash
kubectl create namespace k8s-cloud-tagger
```

## 3. Install with Helm

```bash
helm install k8s-cloud-tagger helm/k8s-cloud-tagger \
  --namespace k8s-cloud-tagger \
  --set cloudProvider=aws \
  --set aws.controllersKubernetes.enabled=true \
  --set aws.controllersKubernetes.accountId="$ACCOUNT_ID" \
  --set aws.controllersKubernetes.oidcIssuerUrl="$OIDC_ISSUER_URL" \
  --set deployment.env.RUST_LOG="debug" \
  --set deployment.env.RUST_BACKTRACE=1
```

The controller pod will start immediately with the role ARN annotation. It may
restart once or twice while ACK creates the IAM role (typically 10-30 seconds).
Once the role is ready, the controller will start tagging PVCs.

## 4. Verify

Check ACK is creating the IAM resources:

```bash
kubectl get role.iam.services.k8s.aws -n k8s-cloud-tagger
kubectl get policy.iam.services.k8s.aws -n k8s-cloud-tagger
```

Check the controller is running:

```bash
kubectl get pods -n k8s-cloud-tagger
```

Check the logs for successful tagging:

```bash
kubectl logs -n k8s-cloud-tagger \
  -l app.kubernetes.io/name=k8s-cloud-tagger \
  --tail=20
```

Look for `AWS: tags applied` in the output.

To verify tags are being set on EBS volumes, create a test PVC:

```bash
cat <<EOF | kubectl apply -f -
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: test-pvc
  namespace: default
  labels:
    environment: test
    team: platform
spec:
  accessModes:
    - ReadWriteOnce
  resources:
    requests:
      storage: 1Gi
EOF
```

Then check the volume tags:

```bash
aws ec2 describe-volumes \
  --filters Name=tag:kubernetes.io/created-for/pvc/name,Values=test-pvc \
  --query 'Volumes[*].Tags'
```

## Troubleshooting

### Pod is crash-looping

This is expected for the first 10-30 seconds while ACK creates the IAM role.
Check if the role exists:

```bash
aws iam get-role --role-name k8s-cloud-tagger
```

Check ACK controller logs:

```bash
kubectl logs -n ack-iam-controller -l app.kubernetes.io/name=iam-controller
```

### IRSA is not working

Check if the pod has the required environment variables:

```bash
kubectl exec -n k8s-cloud-tagger \
  deployment/k8s-cloud-tagger -- env | grep AWS
```

You should see `AWS_WEB_IDENTITY_TOKEN_FILE` and `AWS_ROLE_ARN`.

### Permission errors

Verify the IAM role has the correct trust policy:

```bash
aws iam get-role --role-name k8s-cloud-tagger --query 'Role.AssumeRolePolicyDocument'
```

Check that the inline policy is attached:

```bash
aws iam get-role-policy \
  --role-name k8s-cloud-tagger \
  --policy-name ebs-tagging-policy
```

### Volume not found

Ensure the PVC is bound to a PV:

```bash
kubectl get pvc test-pvc -n default
```

Check that the PV has a CSI volumeHandle in the expected format:

```bash
kubectl get pv <pv-name> -o yaml | grep volumeHandle
```

## Security Considerations

- The current implementation requires account-wide access to all EBS volumes.
  This is because EBS volume ARNs do not support resource-level permissions
  for the CreateTags action.
- ACK resources are managed as Kubernetes resources and will be deleted when
  you run `helm uninstall`.
- Use dedicated node groups for the controller if your security requirements
  mandate isolation.

## Cluster Management

Scale down to zero (stop paying for compute):

```bash
eksctl scale nodegroup \
  --cluster $CLUSTER_NAME \
  --name <nodegroup-name> \
  --nodes 0
```

Scale back up:

```bash
eksctl scale nodegroup \
  --cluster $CLUSTER_NAME \
  --name <nodegroup-name> \
  --nodes 1
```

## Appendix: Manual IAM Setup (without ACK)

If you prefer not to use ACK, you can create the IAM resources manually:

### 1. Create the IAM OIDC Provider

```bash
eksctl utils associate-iam-oidc-provider \
  --cluster $CLUSTER_NAME \
  --region $REGION \
  --approve
```

### 2. Create the IAM Policy

```bash
cat > /tmp/k8s-cloud-tagger-policy.json << 'EOF'
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "ec2:DescribeVolumes",
        "ec2:CreateTags"
      ],
      "Resource": "*"
    }
  ]
}
EOF

aws iam create-policy \
  --policy-name k8s-cloud-tagger \
  --policy-document file:///tmp/k8s-cloud-tagger-policy.json
```

### 3. Create the IAM Role

```bash
eksctl create iamserviceaccount \
  --name k8s-cloud-tagger \
  --namespace k8s-cloud-tagger \
  --cluster $CLUSTER_NAME \
  --region $REGION \
  --role-name k8s-cloud-tagger \
  --attach-policy-arn arn:aws:iam::${ACCOUNT_ID}:policy/k8s-cloud-tagger \
  --approve
```

### 4. Install without ACK

```bash
export ROLE_ARN="arn:aws:iam::${ACCOUNT_ID}:role/k8s-cloud-tagger"

helm install k8s-cloud-tagger helm/k8s-cloud-tagger \
  --namespace k8s-cloud-tagger \
  --set cloudProvider=aws \
  --set serviceAccount.annotations.\"eks\.amazonaws\.com/role-arn\"="$ROLE_ARN"
```
