use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Kubernetes API error: {0}")]
    Kube(#[from] kube::Error),
}

impl Error {
    /// Returns a label-safe string for metrics.
    /// Keep cardinality low — don't use dynamic strings.
    pub fn metric_label(&self) -> &'static str {
        match self {
            Error::Kube(_) => "kube",
        }
    }
}
