VERSION 0.8

ARG --global VERSION=dev
ARG --global IMAGE_DEV=quay.io/upgrades/k8s-cloud-tagger-dev

rust-base:
    FROM rust:1.93
    WORKDIR /app
    RUN rustup component add rustfmt clippy
    COPY --dir src Cargo.toml Cargo.lock .
    CACHE /usr/local/cargo/registry

fmt:
    FROM +rust-base
    RUN cargo fmt --check

build:
    FROM +rust-base
    RUN cargo build --all-targets --tests
    SAVE ARTIFACT target

clippy:
    FROM +rust-base
    COPY +build/target ./target
    RUN cargo clippy -- -D warnings

test:
    FROM +rust-base
    COPY +build/target ./target
    RUN cargo test

ci:
    BUILD +fmt
    BUILD +clippy
    BUILD +test

# --- Release binary (static, amd64) ---

build-release:
    FROM +rust-base
    RUN rustup target add x86_64-unknown-linux-musl
    RUN apt-get update && apt-get install -y musl-tools
    RUN cargo build --release --target x86_64-unknown-linux-musl
    SAVE ARTIFACT target/x86_64-unknown-linux-musl/release/k8s-cloud-tagger

# --- Dev image ---

image-dev:
    FROM cgr.dev/chainguard/static:latest
    COPY +build-release/k8s-cloud-tagger /k8s-cloud-tagger
    USER nonroot:nonroot
    ENTRYPOINT ["/k8s-cloud-tagger"]
    # push is not intended for local dev, only for CI
    # push is ignored locally because --push has to be passed on the command line
    SAVE IMAGE --push ${IMAGE_DEV}:${VERSION}

# --- Dev run ---
dev-up:
    LOCALLY
    BUILD +image-dev
    RUN kubectl apply --kustomize .

dev-down:
    LOCALLY
    RUN kubectl delete --ignore-not-found=true --kustomize .
