use collaboration_central_core::{CentralError, StoreError};
#[derive(Debug, thiserror::Error)]
pub enum LocalError {
    #[error(transparent)]
    Core(#[from] CentralError),
    #[error(transparent)]
    CallEdge(#[from] collaboration_central_core::CallEdgeRefused),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("local IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("local encoding: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("local identity: {0}")]
    Identity(#[from] collaboration_replication_protocol::IdentityError),
    #[error("local task: {0}")]
    Task(String),
    #[error("local transport: {0}")]
    Transport(String),
}
impl From<&str> for LocalError {
    fn from(message: &str) -> Self {
        Self::Transport(message.into())
    }
}
impl From<tokio::task::JoinError> for LocalError {
    fn from(error: tokio::task::JoinError) -> Self {
        Self::Task(error.to_string())
    }
}
impl From<tokio::sync::oneshot::error::RecvError> for LocalError {
    fn from(error: tokio::sync::oneshot::error::RecvError) -> Self {
        Self::Transport(error.to_string())
    }
}
pub type LocalResult<TValue> = Result<TValue, LocalError>;
