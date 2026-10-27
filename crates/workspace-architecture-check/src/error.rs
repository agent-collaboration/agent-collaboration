use std::path::PathBuf;
use std::process::ExitStatus;

#[derive(Debug, thiserror::Error)]
pub enum ArchitectureError {
    #[error("invalid cargo-modules DOT at line {line_number}: {line}")]
    InvalidDot { line_number: usize, line: String },
    #[error("{operation} {}: {source}", path.display())]
    File {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot run {command}: {source}")]
    CommandIo {
        command: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{command} failed ({status}):\n{stderr}")]
    CommandFailed {
        command: String,
        status: ExitStatus,
        stderr: String,
    },
    #[error("{context}: {source}")]
    Json {
        context: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("{command} returned non-UTF-8 output: {source}")]
    OutputEncoding {
        command: String,
        #[source]
        source: std::string::FromUtf8Error,
    },
    #[error("invalid module cycle baseline: {0}")]
    InvalidBaseline(String),
    #[error("expected cargo-modules 0.28.0, found {0}")]
    ToolVersion(String),
    #[error("architecture scan worker panicked")]
    WorkerPanicked,
}
