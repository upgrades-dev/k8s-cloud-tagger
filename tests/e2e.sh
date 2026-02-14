#!/usr/bin/env bash
set -euo pipefail

CLUSTER_NAME="k8s-cloud-tagger-e2e"
NAMESPACE="k8s-cloud-tagger"
KEEP_CLUSTER="${KEEP_CLUSTER:-false}"
IMAGE="${IMAGE:-}"

# ── Helpers ──────────────────────────────────────────────────────────────────

cleanup() {
  if [ "${KEEP_CLUSTER}" = "true" ]; then
    echo ""
    echo "==> Cluster kept: ${CLUSTER_NAME}"
    echo "    kind export kubeconfig --name ${CLUSTER_NAME}"
    echo "    kubectl get pods -A"
    echo "    kind delete cluster --name ${CLUSTER_NAME}  # clean up when done"
  else
    echo "==> Deleting Kind cluster..."
    kind delete cluster --name "${CLUSTER_NAME}" 2>/dev/null || true
  fi
}

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

# ── Cluster lifecycle ────────────────────────────────────────────────────────

echo "==> Creating Kind cluster..."
kind create cluster --name "${CLUSTER_NAME}" --wait 60s

trap cleanup EXIT

# ── Image loading ────────────────────────────────────────────────────────────

IMAGE_REPO="${IMAGE%%:*}"
IMAGE_TAG="${IMAGE##*:}"

if [ -n "${IMAGE_ARCHIVE:-}" ]; then
  PULL_POLICY="Never"
  echo "==> Loading local image into Kind..."
  kind load image-archive "${IMAGE_ARCHIVE}" --name "${CLUSTER_NAME}"
else
  PULL_POLICY="IfNotPresent"
  echo "==> Using remote image: ${IMAGE}"
fi

# ── Deploy controller in test mode ──────────────────────────────────────────

echo "==> Installing Helm chart (cloudProvider=test)..."
helm install k8s-cloud-tagger "${CHART_PATH}" \
  --namespace "${NAMESPACE}" \
  --create-namespace \
  --set cloudProvider=test \
  --set image.repository="${IMAGE_REPO}" \
  --set image.tag="${IMAGE_TAG}" \
  --set image.pullPolicy="${PULL_POLICY}" \
  --set deployment.env.RUST_LOG="debug" \
  --wait \
  --timeout 60s

echo "==> Controller is ready."

# ── Create test PVC ─────────────────────────────────────────────────────────

echo "==> Creating test PVC..."
kubectl apply -f "${FIXTURES_PATH}/pvc.yaml"
kubectl apply -f "${FIXTURES_PATH}/pvc-consumer.yaml"

echo "==> Waiting for PVC to bind..."
kubectl wait --for=jsonpath='{.status.phase}'=Bound \
  pvc/test-pvc -n default --timeout=30s

# ── Assert event ─────────────────────────────────────────────────────────────

echo "==> Waiting for Tagged event..."

TIMEOUT=30
INTERVAL=2
ELAPSED=0

while [ "${ELAPSED}" -lt "${TIMEOUT}" ]; do
  EVENTS=$(kubectl get events -n default \
    --field-selector "reason=Tagged,involvedObject.name=test-pvc" \
    -o json)

  COUNT=$(echo "${EVENTS}" | jq '.items | length')

  if [ "${COUNT}" -gt 0 ]; then
    echo "==> Tagged event found:"
    echo "${EVENTS}" | jq -r '.items[0] | "  Type:    \(.type)\n  Reason:  \(.reason)\n  Action:  \(.action)\n  Message: \(.message)"'
    echo ""
    echo "PASS: integration test succeeded."
    exit 0
  fi

  sleep "${INTERVAL}"
  ELAPSED=$((ELAPSED + INTERVAL))
done

# ── Failure diagnostics ─────────────────────────────────────────────────────

echo ""
echo "==> Diagnostics:"
echo "--- Controller logs ---"
kubectl logs -n "${NAMESPACE}" -l app.kubernetes.io/name=k8s-cloud-tagger --tail=50
echo "--- All events in default namespace ---"
kubectl get events -n default
echo ""

fail "Tagged event not found within ${TIMEOUT}s"
