use std::io;

#[derive(Debug, thiserror::Error)]
pub(crate) enum OracleError {
    #[error("legacy source must be a regular file inside the declared copy root")]
    InvalidLegacySourcePath,
    #[error("legacy source is under normal Codex or Router state")]
    NormalStateSourcePath,
    #[error("legacy source has a SQLite sidecar and is not sealed")]
    UnsealedLegacySource,
    #[error("legacy source bytes changed during oracle reading")]
    LegacySourceChanged,
    #[error("legacy source path cannot be represented as a SQLite URI")]
    UnsupportedLegacySourcePath,
    #[error(
        "oracle output path must be absolute and inside an owner-private directory under ~/.cache"
    )]
    InvalidOutputDirectory,
    #[error("oracle output file already exists")]
    OutputAlreadyExists,
    #[error("legacy watch active value must be 0 or 1, got {0}")]
    InvalidWatchActiveValue(i64),
    #[error("oracle count exceeds the supported JSON integer range")]
    CountOutOfRange,
    #[error("oracle filesystem operation failed")]
    Io(#[from] io::Error),
    #[error("legacy SQLite operation failed")]
    Sqlite(#[from] rusqlite::Error),
    #[error("oracle JSON serialization failed")]
    Json(#[from] serde_json::Error),
}
