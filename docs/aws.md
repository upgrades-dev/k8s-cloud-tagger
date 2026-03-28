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

## Prerequisites

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

### IRSA not working

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

## Manual IAM Setup (without ACK)

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
