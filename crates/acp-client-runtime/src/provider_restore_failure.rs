//! A restore failure carries whether the request could have reached ACP.

use crate::ExternalProviderRuntimeError;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

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

    pub(crate) fn replacing_error(mut self, error: ExternalProviderRuntimeError) -> Self {
        *self.error = error;
        self
    }
}

/// Shared with the caller in case a closing connection drops its reply.
#[derive(Clone, Default)]
pub(crate) struct RestoreDispatch(Arc<AtomicBool>);

impl RestoreDispatch {
    pub(crate) fn begin_attempt(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(crate) fn failure(&self, error: ExternalProviderRuntimeError) -> RestoreFailure {
        if self.0.load(Ordering::Acquire) {
            RestoreFailure::attempted(error)
        } else {
            RestoreFailure::preflight(error)
        }
    }
}
