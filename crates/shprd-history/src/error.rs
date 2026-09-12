use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("unsupported agent: {0}")]
    UnsupportedAgent(String),
    #[error("history entry is no longer available")]
    MissingEntry,
    #[error("session file unavailable")]
    MissingFile,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("remote access failed: {0}")]
    Remote(String),
}
pub type Result<T> = std::result::Result<T, Error>;
pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
