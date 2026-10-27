use crate::profile_projection::ProfileGuardError;

#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Profile(#[from] ProfileGuardError),
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Runtime(&'static str),
    #[error("{0}")]
    TimedOut(&'static str),
    #[error("command exited with status {0}")]
    Command(i32),
    #[error("KeyboardInterrupt")]
    Interrupted,
}

impl DriverError {
    #[must_use]
    pub fn category(&self) -> &'static str {
        match self {
            Self::Io(error) => match error.kind() {
                std::io::ErrorKind::NotFound => "FileNotFoundError",
                std::io::ErrorKind::PermissionDenied => "PermissionError",
                _ => "OSError",
            },
            Self::Profile(error) => error.category(),
            Self::Json(_) => "ValidationError",
            Self::Http(_) => "OSError",
            Self::Invalid(_) => "ValueError",
            Self::Runtime(_) => "RuntimeError",
            Self::TimedOut(_) => "TimeoutError",
            Self::Command(_) => "CalledProcessError",
            Self::Interrupted => "KeyboardInterrupt",
        }
    }
}

pub type DriverResult<TValue> = Result<TValue, DriverError>;
