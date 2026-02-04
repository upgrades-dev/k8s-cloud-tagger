VERSION 0.8

rust-base:
    FROM rust:1.93
    WORKDIR /app
    RUN rustup component add rustfmt clippy
    COPY --dir src Cargo.toml Cargo.lock .
    CACHE /usr/local/cargo/registry

fmt:
    FROM +rust-base
    RUN cargo fmt --check

build:
    FROM +rust-base
    RUN cargo build --all-targets --tests
    SAVE ARTIFACT target

clippy:
    FROM +rust-base
    COPY +build/target ./target
    RUN cargo clippy -- -D warnings

test:
    FROM +rust-base
    COPY +build/target ./target
    RUN cargo test

ci:
    BUILD +fmt
    BUILD +clippy
    BUILD +test
