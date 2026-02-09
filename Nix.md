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

# Avoid rate limits
echo "access-tokens = github.com=$(gh auth token)" >> ~/.config/nix/nix.conf

# If you get a ton of 429 or 500 errors
nix flake check --refresh

# Success
Ignoring warnings, this is what success looks like.

```
nix flake check 
Use '--all-systems' to check all.
```