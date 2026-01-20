use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Kubernetes API error: {0}")]
    Kube(#[from] kube::Error),

    #[error("Missing object metadata")]
    MissingMetadata,
}
