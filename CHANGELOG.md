# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Release detection now based on version file diff instead of commit message

## [0.2.0] - 2025-02-25

### Added
- Kubernetes operator that propagates PVC labels to GCP persistent disks
- GCP label sanitisation (lowercasing, replacing invalid characters with hyphens)
- Helm chart for deployment
- Prometheus metrics for reconciliation and cloud API calls
- Health and readiness endpoints
- Kubernetes Events published on successful tagging
- Nix dev shell for reproducible development environments
- Dev image published to Quay.io
- Integration tests
- `cargo xtask` release tooling with version sync enforcement
- GitHub Actions builds, tags and pushes release image to Quay.io
