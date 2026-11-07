//! Errors at the prepared-artifact boundary, independent of its file and export owners.
#[derive(Debug, thiserror::Error)]
pub enum ProjectBundleError {
    #[error("project bundle is incomplete")]
    Incomplete,
    #[error("cutover artifact path is not an owned private path")]
    UnsafePath,
    #[error("prepared artifact is invalid or differs from its seal")]
    InvalidArtifact,
    #[error("source files changed during preparation")]
    SourceChanged,
    #[error("preserved Fleet graph does not contain the selected project's parents")]
    InvalidGraph,
    #[error("cutover identities or target origin are invalid")]
    InvalidPreparation,
    #[error("insufficient free space for the frozen project bundle")]
    InsufficientSpace { available: u64, required: u64 },
    #[error("cannot inspect native source")]
    Source(#[from] crate::NativeSourceError),
    #[error("cannot reframe native source")]
    Transport(#[from] crate::SnapshotTransportError),
    #[error("cannot seal prepared content")]
    Digest(#[from] fleet_protocol::DigestError),
    #[error("cannot encode prepared artifact")]
    Json(#[from] serde_json::Error),
    #[error("cannot persist prepared artifact")]
    Io(#[from] std::io::Error),
}
