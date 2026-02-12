{
  description = "k8s-cloud-tagger - Kubernetes resources tagged in your cloud provider";

  # ============================================================================
  # INPUTS
  # External dependencies (like package.json or go.mod for Nix)
  # ============================================================================
  inputs = {
    # Nix packages collection - provides pkgs.dockerTools, pkgs.cacert, etc.
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    # Crane - best-in-class Nix library for building Rust projects
    # Handles cargo workspace caching, incremental builds, etc.
    # https://github.com/ipetkov/crane
    crane.url = "github:ipetkov/crane";

    # Fenix - provides Rust toolchains for Nix (like rustup, but Nix-native)
    # https://github.com/nix-community/fenix
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs"; # Use our nixpkgs, not fenix's
    };

    # Utility for multi-system support (x86_64-linux, aarch64-darwin, etc.)
    flake-utils.url = "github:numtide/flake-utils";
  };

  # ============================================================================
  # OUTPUTS
  # What this flake provides: checks, packages, devShells
  # ============================================================================
  outputs = { self, nixpkgs, crane, fenix, flake-utils, ... }:
    flake-utils.lib.eachSystem [ "x86_64-linux" ] (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        # ----------------------------------------------------------------------
        # Rust Toolchains
        # ----------------------------------------------------------------------

        # Standard toolchain with dev tools (fmt, clippy)
        # Used for: checks, development, default builds
        toolchain = fenix.packages.${system}.stable.withComponents [
          "cargo"
          "clippy"
          "rustc"
          "rustfmt"
        ];

        # Musl cross-compilation toolchain
        # Used for: static binary builds (no glibc dependency)
        # This produces binaries that run on minimal containers (scratch, chainguard/static)
        toolchainMusl = with fenix.packages.${system}; combine [
          stable.cargo
          stable.rustc
          targets.x86_64-unknown-linux-musl.stable.rust-std
        ];

        # ----------------------------------------------------------------------
        # Crane Build Library
        # ----------------------------------------------------------------------

        # Standard crane instance for checks and dev builds
        craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

        # Musl-targeting crane instance for release builds
        craneLibMusl = (crane.mkLib pkgs).overrideToolchain toolchainMusl;

        # ----------------------------------------------------------------------
        # Source Filtering
        # ----------------------------------------------------------------------

        # cleanCargoSource filters out non-Rust files (docs, CI configs, etc.)
        # This improves caching - changes to README.md won't trigger rebuilds
        src = craneLib.cleanCargoSource ./.;

        # Shared arguments for all cargo invocations
        commonArgs = {
          inherit src;
          strictDeps = true; # Prevents impure build dependencies
        };

        # ----------------------------------------------------------------------
        # Build Artifacts
        # ----------------------------------------------------------------------

        # Pre-build dependencies only (cached separately from source changes)
        # This is the key to fast incremental builds:
        # - Change Cargo.toml/Cargo.lock -> rebuild deps + source
        # - Change src/*.rs -> reuse cached deps, rebuild source only
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        # Cached deps for musl toolchain (used by static binary)
        cargoArtifactsMusl = craneLibMusl.buildDepsOnly (commonArgs // {
          CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";
          CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static";
        });

        # Static musl binary for container images
        # - No glibc dependency
        # - Runs on scratch/distroless/chainguard-static bases
        binaryMusl = craneLibMusl.buildPackage (commonArgs // {
          cargoArtifacts = cargoArtifactsMusl;
          CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";
          CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static";
        });

        # ----------------------------------------------------------------------
        # Chainguard Static Base Image (pinned)
        # Update with: nix-prefetch-docker --image-name cgr.dev/chainguard/static --image-tag latest
        # ----------------------------------------------------------------------
        chainguardStatic = pkgs.dockerTools.pullImage {
          imageName = "cgr.dev/chainguard/static";
          imageDigest = "sha256:9cef3c6a78264cb7e25923bf1bf7f39476dccbcc993af9f4ffeb191b77a7951e";
          hash = "sha256-0/N09XBMjLil6X9yQMczPi3NYEk31/g8Ghmm7TRXsdc=";
          finalImageName = "cgr.dev/chainguard/static";
          finalImageTag = "latest";
        };

      in
      {
        # ======================================================================
        # CHECKS
        # Run with: nix flake check
        # These run in parallel and fail fast
        # ======================================================================
        checks = {
          # cargo fmt --check
          fmt = craneLib.cargoFmt { inherit src; };

          # cargo clippy -- -D warnings
          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts; # Reuse cached deps
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          });

          # cargo test
          test = craneLib.cargoTest (commonArgs // {
            inherit cargoArtifacts; # Reuse cached deps
          });
        };

        # ======================================================================
        # PACKAGES
        # Build with: nix build .#<package-name>
        # ======================================================================
        packages = {
          # Default package (dynamically linked, for local dev)
          # Build with: nix build
          default = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
          });

          # Static binary for containers
          # Build with: nix build .#binary-static
          binary-static = binaryMusl;

          # OCI container image (tarball) based on Chainguard static
          # Build with: nix build .#image-dev
          # Push with: skopeo copy docker-archive:result docker://quay.io/...
          image-dev = pkgs.dockerTools.buildImage {
            name = "quay.io/upgrades/k8s-cloud-tagger-dev";
            tag = "dev";

            # Chainguard static base (provides CA certs, tzdata, nonroot user)
            fromImage = chainguardStatic;

            # Container configuration
            config = {
              Entrypoint = [ "${binaryMusl}/bin/k8s-cloud-tagger" ];
              User = "nonroot:nonroot";
            };
          };
        };

        # ======================================================================
        # DEV SHELL
        # Enter with: nix develop
        # Provides: cargo, rustc, rustfmt, clippy, rust-analyzer
        # ======================================================================
        devShells.default = craneLib.devShell {
          # Include all check inputs (gives you the same tools CI uses)
          checks = self.checks.${system};

          # Additional packages for development
          packages = with pkgs; [
            nix-prefetch-docker  # Provides nix-prefetch-docker
            skopeo # Push images without Docker daemon
          ];

          # Message of the day
          shellHook = ''
            cat << 'EOF'

            __  _____  ________  ___   ___  ________
           / / / / _ \/ ___/ _ \/ _ | / _ \/ __/ __/
          / /_/ / ___/ (_ / , _/ __ |/ // / _/_\ \
          \____/_/   \___/_/|_/_/ |_/____/___/___/

          EOF
            echo "entering k8s-cloud-tagger dev shell..."
          '';

        };
      });
}
