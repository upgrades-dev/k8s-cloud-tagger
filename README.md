# K8s-Cloud-Tagger

Kubernetes cloud tagger watches cluster resources and applies labels in your cloud provider.

## Develop

### Install prerequisites

The following prerequisites are expected to be installed on your system already:

* [Nix](https://nix.dev/install-nix.html)
* [Rust](https://rust-lang.org/tools/install)
* [Docker Desktop](https://docs.docker.com/desktop/use-desktop/)
* [Kubernetes](https://kubernetes.io/docs/setup/)

### Test

#### Unit tests

```bash
cargo test
```

## Run

### Dev

Run all CI tasks:

```bash
nix build
```
