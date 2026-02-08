# Usage

```bash
# Development
earthly +fmt
earthly +clippy
earthly +test

# CI (checks only)
earthly +ci

# CI + image (PRs, main branch)
earthly +ci-all --VERSION=sha-$(git rev-parse --short HEAD)

# Dev image (local)
earthly --push +image-dev --VERSION=local-$USER

# Production release
earthly --push +image-prod --VERSION=v1.2.3
```

## Summary

| Target | What it runs |
| --- | --- |
| `+ci` | fmt, clippy, test |
| `+ci-all` | ci + sbom + dev image |
| `+image-dev` | Dev image only |
| `+image-prod` | Prod image (releases) |