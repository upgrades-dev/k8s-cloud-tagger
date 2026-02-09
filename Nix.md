# Nix Build System

This project uses [Nix Flakes](https://nixos.wiki/wiki/Flakes) for reproducible builds.

## Prerequisites

### Enable Flakes

```bash
# Add to ~/.config/nix/nix.conf (create if doesn't exist)
echo "experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf
```

### Avoid GitHub Rate Limits

Nix fetches flake inputs from GitHub. Without auth, you'll hit rate limits.

```bash
# If you have gh CLI installed
echo "access-tokens = github.com=$(gh auth token)" >> ~/.config/nix/nix.conf

# Or with a personal access token
echo "access-tokens = github.com=ghp_your_token_here" >> ~/.config/nix/nix.conf
```

### Recovery from Rate Limits

If you get repeated 429 or 500 errors:

```bash
nix flake check --refresh
```

---

## Commands

### CI Checks

```bash
# Run all CI checks (fmt + clippy + test) - equivalent to `earthly +ci`
nix flake check

# Run checks individually
nix build .#checks.x86_64-linux.fmt
nix build .#checks.x86_64-linux.clippy
nix build .#checks.x86_64-linux.test
```

**Success looks like this** (warnings are informational):

```
$ nix flake check 
warning: Git tree '/path/to/project' is dirty
warning: The check omitted these incompatible systems: aarch64-darwin, aarch64-linux, x86_64-darwin
Use '--all-systems' to check all.
```

No output = all checks passed. Nix only prints on failure.

### Build Binary

```bash
# Build default binary (dynamically linked, for local dev)
nix build

# Build static musl binary (for containers)
nix build .#binary-static

# Binary is at ./result/bin/k8s-cloud-tagger
./result/bin/k8s-cloud-tagger --help
```

### Build Container Image

```bash
# Build OCI image as tarball
nix build .#image-dev

# Result is a tarball at ./result
file result
# result: symbolic link to /nix/store/...-docker-image-k8s-cloud-tagger-dev.tar.gz
```

### Dev Shell

```bash
# Enter development environment with all tools
nix develop

# Now you have: cargo, rustc, rustfmt, clippy, skopeo
cargo --version
skopeo --version
```

---

## Pushing Images

Nix builds images as tarballs. To push to a registry, use **skopeo**.

### What is Skopeo?

[Skopeo](https://github.com/containers/skopeo) is a CLI tool for working with container images **without a Docker daemon**:

| Task | Skopeo | Docker |
| --- | --- | --- |
| Copy images between registries | ✅ | ✅ (pull + push) |
| Push from tarball | ✅ | `docker load` + `push` |
| Inspect remote images | ✅ | ✅ |
| Delete remote tags | ✅ | ❌ |
| Requires daemon | ❌ | ✅ |

### Manual Push

```bash
# Build the image
nix build .#image-dev

# Login to registry (creates ~/.config/containers/auth.json)
skopeo login quay.io -u your-username

# Push tarball to registry
skopeo copy docker-archive:result docker://quay.io/upgrades/k8s-cloud-tagger-dev:v1.0.0
```

### Push Script

Create `scripts/push-dev.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-sha-$(git rev-parse --short HEAD)}"
IMAGE="quay.io/upgrades/k8s-cloud-tagger-dev:${VERSION}"

echo "Building image..."
nix build .#image-dev

echo "Pushing ${IMAGE}..."
skopeo copy docker-archive:result "docker://${IMAGE}"

echo "✅ Pushed ${IMAGE}"
```

Usage:

```bash
chmod +x scripts/push-dev.sh

# Push with git SHA
./scripts/push-dev.sh

# Push with custom version
./scripts/push-dev.sh v1.2.3
```

### Alternative: Load into Docker

If you prefer using Docker:

```bash
nix build .#image-dev
docker load < result
docker push quay.io/upgrades/k8s-cloud-tagger-dev:dev
```

---

## GitHub Actions

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
  workflow_dispatch:

jobs:
  ci:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: cachix/install-nix-action@v27
        with:
          github_access_token: ${{ secrets.GITHUB_TOKEN }}

      - name: Run checks
        run: nix flake check

  push-dev:
    runs-on: ubuntu-latest
    needs: ci
    if: github.ref == 'refs/heads/main'
    steps:
      - uses: actions/checkout@v4

      - uses: cachix/install-nix-action@v27
        with:
          github_access_token: ${{ secrets.GITHUB_TOKEN }}

      - name: Build image
        run: nix build .#image-dev

      - name: Push to Quay
        run: |
          skopeo login quay.io -u ${{ secrets.QUAY_USERNAME }} -p ${{ secrets.QUAY_PASSWORD }}
          skopeo copy docker-archive:result docker://quay.io/upgrades/k8s-cloud-tagger-dev:sha-${{ github.sha }}
```

---

## TODO

### Use Chainguard Base Image

Currently, `image-dev` uses a Nix-built scratch image with just CA certificates. For production, consider using [Chainguard's static image](https://images.chainguard.dev/directory/image/static/overview) which provides:

- Pre-signed images (Cosign)
- Daily CVE scanning and rebuilds
- SBOM included
- Zero known CVEs policy

Options to explore:

1. **Hybrid approach**: Use Nix to build the binary, copy into Chainguard base via Dockerfile
2. **OCI base layers**: Use `pkgs.dockerTools.pullImage` to pull Chainguard as a base (complex)
3. **Accept Nix-native**: The current approach is fully reproducible and minimal (~3MB)

For now, the Nix-native image is sufficient for dev. Evaluate Chainguard for production releases.

---

## Comparison: Earthly vs Nix

| Earthly | Nix |
| --- | --- |
| `earthly +ci` | `nix flake check` |
| `earthly +build-release` | `nix build .#binary-static` |
| `earthly +image-dev` | `nix build .#image-dev` |
| `earthly --push +image-dev --VERSION=v1` | `./scripts/push-dev.sh v1` |
