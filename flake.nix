{
  description = "k8s-cloud-tagger - Kubernetes PVC tagging controller";

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
    flake-utils.lib.eachDefaultSystem (system:
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

        # Static musl binary for container images
        # - No glibc dependency
        # - Runs on scratch/distroless/chainguard-static bases
        binaryMusl = craneLibMusl.buildPackage (commonArgs // {
          CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";
          CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static";
        });

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

          # OCI container image (tarball)
          # Build with: nix build .#image-dev
          # Load with: docker load < result
          # Push with: skopeo copy docker-archive:result docker://quay.io/...
          image-dev = pkgs.dockerTools.buildImage {
            name = "quay.io/upgrades/k8s-cloud-tagger-dev";
            tag = "dev";

            # Image contents (minimal - just CA certs for TLS)
            copyToRoot = pkgs.buildEnv {
              name = "image-root";
              paths = [
                pkgs.cacert # CA certificates for HTTPS connections
              ];
              pathsToLink = [ "/etc" ];
            };

            # Container configuration
            config = {
              Entrypoint = [ "${binaryMusl}/bin/k8s-cloud-tagger" ];
              User = "65532:65532"; # nonroot (matches chainguard/distroless convention)
              Env = [
                "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt"
              ];
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
