# Dagger CI/CD Pipeline

This directory contains the CI/CD pipeline written in Rust using the [dagger-sdk](https://docs.rs/dagger-sdk) crate.

## Why Rust with Dagger?

Dagger provides native SDKs for Go, Python, TypeScript, PHP, Java, .NET, Elixir, and Rust. 

The Rust SDK is a **client library** for calling the Dagger API – it doesn't yet support Dagger modules (`dagger call`).

This means we run the pipeline as a standard Rust binary rather than using `dagger call`.

## Running Locally

```bash
cd dagger
cargo run
```

Example output:

```
running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

✅ Tests passed
```

## GitHub Actions

We use [actions-rust-lang/setup-rust-toolchain](https://github.com/actions-rust-lang/setup-rust-toolchain) instead of [dtolnay/rust-toolchain](https://github.com/dtolnay/rust-toolchain). Both are excellent choices:

- **dtolnay/rust-toolchain** — Minimal, reliable, maintained by David Tolnay (author of `serde`, `syn`, etc.)
- **actions-rust-lang/setup-rust-toolchain** — Extends dtolnay's action with **problem matchers** (compiler errors appear inline in PRs) and optional caching

We chose `actions-rust-lang` for the problem matchers—they surface `cargo` errors directly in pull request diffs, improving code review.

## Future

When Dagger adds full Rust module support, this pipeline can be invoked via:

```bash
dagger call test
dagger call build
dagger call push
```

Until then, `cargo run` works identically locally and in CI.
