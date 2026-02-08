VERSION 0.8

IMPORT ./build/rust AS rust
IMPORT ./build/image AS image
IMPORT ./build/ci AS ci

# Dev commands
fmt:
    BUILD rust+fmt

clippy:
    BUILD rust+clippy

test:
    BUILD rust+test

# CI
ci:
    BUILD ci+ci

ci-all:
    BUILD ci+ci-all

# Images
image-dev:
    ARG VERSION=dev
    BUILD image+dev --VERSION=${VERSION}

image-prod:
    ARG VERSION
    BUILD image+prod --VERSION=${VERSION}

# Artifacts
sbom:
    BUILD ci+sbom
