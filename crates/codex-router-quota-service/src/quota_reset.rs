//! Integrated, live-only usage-limit reset workflow.

use thiserror::Error;

pub mod credential_authority;
pub mod provider_protocol;
mod quota_reset_session_composition;
pub mod reset_commit_service;
pub mod reset_credit_policy;
pub mod reset_session_supervisor;
pub mod reset_workflow_reducer;

pub use quota_reset_session_composition::FixedOriginInteractiveResetSessionFactory;
pub use quota_reset_session_composition::InteractiveResetSession;
pub use quota_reset_session_composition::InteractiveResetSessionFactory;
#[cfg(feature = "quota-reset-test-harness")]
pub use quota_reset_session_composition::LoopbackInteractiveResetSessionFactory;

/// Sanitized reset-provider composition and protocol failure.
#[derive(Debug, Error)]
pub enum QuotaResetError {
    #[error("Keychain locked or access denied")]
    CredentialStoreKeyUnavailable,
    #[error("credential migration is incomplete for accounts: {accounts:?}")]
    CredentialMigrationIncomplete { accounts: Vec<String> },
    #[error("encrypted credential store could not be initialized")]
    CredentialStoreUnavailable,
    #[error("quota reset provider request failed: {message}")]
    Request { message: String },
    #[error("quota reset provider returned HTTP {status}")]
    Status { status: u16 },
    #[error("quota reset provider response was unusable: {message}")]
    Response { message: String },
}
