VERSION 0.8

# ─────────────────────────────────────────────────────────────
# Base images
# ─────────────────────────────────────────────────────────────

rust-base:
    FROM rust:1.84
    WORKDIR /app

# ─────────────────────────────────────────────────────────────
# Unit and feature tests (cargo test)
# ─────────────────────────────────────────────────────────────

test:
    FROM +rust-base
    COPY --dir src Cargo.toml Cargo.lock .
    RUN cargo test

fmt:
    FROM +rust-base
    COPY --dir src Cargo.toml Cargo.lock .
    RUN cargo fmt --check

clippy:
    FROM +rust-base
    COPY --dir src Cargo.toml Cargo.lock .
    RUN cargo clippy -- -D warnings

# ─────────────────────────────────────────────────────────────
# Build release binary
# ─────────────────────────────────────────────────────────────

build:
    FROM +rust-base
    COPY --dir src Cargo.toml Cargo.lock .
    RUN cargo build --release
    SAVE ARTIFACT target/release/k8s-cloud-tagger

# ─────────────────────────────────────────────────────────────
# Container image
# ─────────────────────────────────────────────────────────────

image:
    FROM gcr.io/distroless/cc-debian12
    COPY +build/k8s-cloud-tagger /usr/local/bin/
    ENTRYPOINT ["/usr/local/bin/k8s-cloud-tagger"]
    SAVE IMAGE k8s-cloud-tagger:latest

# ─────────────────────────────────────────────────────────────
# Integration test (Kind)
# ─────────────────────────────────────────────────────────────

integration-test:
    FROM earthly/dind:alpine-3.19
    RUN apk add --no-cache curl bash
    RUN curl -Lo /usr/local/bin/kind https://kind.sigs.k8s.io/dl/v0.27.0/kind-linux-amd64 && chmod +x /usr/local/bin/kind
    RUN curl -Lo /usr/local/bin/kubectl https://dl.k8s.io/release/v1.32.0/bin/linux/amd64/kubectl && chmod +x /usr/local/bin/kubectl
    COPY manifests /manifests
    WITH DOCKER --load k8s-cloud-tagger:latest=+image
        RUN kind create cluster --name test && \
            kind load docker-image k8s-cloud-tagger:latest --name test && \
            kubectl apply -f /manifests/ && \
            kubectl wait --for=condition=ready pod -l app=k8s-cloud-tagger --timeout=60s && \
            kubectl logs -l app=k8s-cloud-tagger && \
            kind delete cluster --name test
    END

# ─────────────────────────────────────────────────────────────
# CI pipeline (runs everything)
# ─────────────────────────────────────────────────────────────

ci:
    BUILD +fmt
    BUILD +clippy
    BUILD +test
    BUILD +integration-test
