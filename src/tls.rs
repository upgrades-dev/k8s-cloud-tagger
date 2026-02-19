//! TLS configuration for outbound HTTPS connections.
//!
//! This module builds a rustls `ClientConfig` that:
//!
//! 1. Loads system CA certificates (e.g. `/etc/ssl/certs`), which picks up
//!    private CAs mounted into the container.
//! 2. Always includes Mozilla's bundled root certificates as a baseline,
//!    so public cloud APIs work even in minimal containers with no system
//!    cert store.
//!
//! We use `ring` as the cryptography backend instead of the default
//! `aws-lc-rs`. This avoids a C/ASM dependency (libaws_lc_sys) that
//! requires glibc and breaks in static/musl/Nix builds.

use rustls::{ClientConfig, RootCertStore};

/// Install `ring` as the global rustls crypto provider.
///
/// Must be called once at startup before any TLS connections are made.
/// Panics if a provider has already been installed.
pub fn install_crypto_provider() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
}

/// Build a rustls `ClientConfig` with system + bundled Mozilla CA roots.
pub fn client_config() -> ClientConfig {
    ClientConfig::builder()
        .with_root_certificates(root_cert_store())
        .with_no_client_auth()
}

/// Build a `reqwest::Client` using our TLS configuration.
#[allow(dead_code)] // TODO(afharvey) issue 14 will use this
pub fn http_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .use_preconfigured_tls(client_config())
        .build()
}

#[allow(dead_code)] // TODO(afharvey) issue 14 will use this
fn root_cert_store() -> RootCertStore {
    let mut roots = RootCertStore::empty();

    // System certs — picks up private CAs mounted into the container.
    let native = rustls_native_certs::load_native_certs();

    if let Some(err) = native.errors.first() {
        tracing::warn!(
            %err,
            "Error loading some system certs, bundled Mozilla roots will fill gaps"
        );
    }

    let (added, failed) = roots.add_parsable_certificates(native.certs);
    tracing::debug!(added, failed, "Loaded system CA certificates");

    // Bundled Mozilla roots — always present as a baseline.
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    roots
}
