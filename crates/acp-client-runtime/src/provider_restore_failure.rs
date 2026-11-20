//! A restore failure carries whether the request could have reached ACP.

use crate::ExternalProviderRuntimeError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestorePhase {
    Preflight,
    Attempted,
}

#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct RestoreFailure {
    pub phase: RestorePhase,
    #[source]
    error: Box<ExternalProviderRuntimeError>,
}

impl RestoreFailure {
    #[must_use]
    pub fn preflight(error: ExternalProviderRuntimeError) -> Self {
        Self {
            phase: RestorePhase::Preflight,
            error: Box::new(error),
        }
    }

    #[must_use]
    pub fn attempted(error: ExternalProviderRuntimeError) -> Self {
        Self {
            phase: RestorePhase::Attempted,
            error: Box::new(error),
        }
    }

    #[must_use]
    pub fn error(&self) -> &ExternalProviderRuntimeError {
        self.error.as_ref()
    }

    #[must_use]
    pub fn into_error(self) -> ExternalProviderRuntimeError {
        *self.error
    }
}
