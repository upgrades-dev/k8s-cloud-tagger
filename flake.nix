{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, fenix, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        # Rust toolchain with fmt + clippy
        toolchain = fenix.packages.${system}.stable.withComponents [
          "cargo"
          "clippy"
          "rustc"
          "rustfmt"
        ];

        craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

        # Source filtering (only Rust-relevant files)
        src = craneLib.cleanCargoSource ./.;

        # Common args
        commonArgs = {
          inherit src;
          strictDeps = true;
        };

        # Build deps only (cached layer, like your +build target)
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      in
      {
        # `nix flake check` runs all of these
        checks = {
          # cargo fmt --check
          fmt = craneLib.cargoFmt {
            inherit src;
          };

          # cargo clippy -- -D warnings
          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          });

          # cargo test
          test = craneLib.cargoTest (commonArgs // {
            inherit cargoArtifacts;
          });
        };

        # `nix build` produces the binary
        packages.default = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        });

        # `nix develop` drops you into a shell with tooling
        devShells.default = craneLib.devShell {
          checks = self.checks.${system};
        };
      });
}
