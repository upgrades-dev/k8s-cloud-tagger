# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-04-01

### Added

- AWS EBS tagging and a testing guide for EKS
- Optional Helm [ACK](https://aws-controllers-k8s.github.io/community/docs/community/overview/) resources for EKS
- New nix shell with AWS tools "nix develop .#aws"

## [0.3.0] - 2026-03-27

### Added

- Optional Helm [Config Connector](https://docs.cloud.google.com/config-connector/docs/overview) resources for GKE
- Determine cloud provider from pv csi driver name
- Optional Helm [ASO](https://azure.github.io/azure-service-operator) Azure Service Operator resources for AKS
- Azure Disk labelling and a testing guide for AKS

## [0.2.1] - 2026-02-26

### Fixed

- Release detection now based on version file diff instead of commit message

### Added

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
