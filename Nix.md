# Run all CI checks (fmt + clippy + test)
nix flake check

# Run individually
nix build .#checks.x86_64-linux.fmt
nix build .#checks.x86_64-linux.clippy
nix build .#checks.x86_64-linux.test

# Build binary
nix build

# Dev shell with all tools
nix develop

# Enable flakes (if not already)
echo "experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf
