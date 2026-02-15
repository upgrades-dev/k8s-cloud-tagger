use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Kubernetes API error: {0}")]
    Kube(#[from] kube::Error),

    #[error("GCP auth error: {0}")]
    Gcp(#[from] gcp_auth::Error),

    #[error("HTTP error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[allow(dead_code)] // Used in tests
    #[error("Cloud API error: {0}")]
    CloudApi(String),
}

impl Error {
    /// Returns a label-safe string for metrics.
    /// Keep cardinality low — don't use dynamic strings.
    pub fn metric_label(&self) -> &'static str {
        match self {
            Error::Kube(_) => "kube",
            Error::Gcp(_) => "gcp",
            Error::Reqwest(_) => "http",
            Error::CloudApi(_) => "cloud_api",
        }
    }
}
