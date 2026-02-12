# K8s-Cloud-Tagger

Kubernetes cloud tagger watches cluster resources and applies labels in your cloud provider.

## Develop

### Install prerequisites

The following prerequisites are expected to be installed on your system already:

* [Rust](https://rust-lang.org/tools/install)
* [Docker Desktop](https://docs.docker.com/desktop/use-desktop/)
* [Kubernetes](https://kubernetes.io/docs/setup/)
* Earthly

#### Installing Earthly

Earthly was abandoned in early 2025, and the [community fork](https://www.earthbuild.dev/) is still starting up. In the meantime, download the final release from https://github.com/earthly/earthly/releases.

### Test

#### Unit tests

```
cargo test
```

#### Integration tests

```
earthly +ci
```

## Run

### Dev

Build the dev image and deploy to your Kubernetes cluster:

```
earthly +dev-up
```

Delete pod and all other resources

```
earthly +dev-down
```
