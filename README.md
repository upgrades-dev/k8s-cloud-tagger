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

#### Run an e2e test locally

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

## Helm

To get the raw Kubernetes manifests:

```bash
nix develop
helm template k8s-cloud-tagger helm/k8s-cloud-tagger/ --set serviceMonitor.enabled=true
```
