//! Quota service failures.
use codex_router_auth::resolver::CredentialResolverError;
use codex_router_state::sqlite::StateStoreError;
use thiserror::Error;
/// Quota command failure.
#[derive(Debug, Error)]
pub enum QuotaServiceError {
    /// Quota refresh base URL is not one of the allowlisted provider URLs.
    #[error("quota refresh base URL is not allowed: {base_url}")]
    DisallowedBaseUrl {
        /// Rejected base URL.
        base_url: String,
    },
    /// Quota refresh is not implemented for allowed providers in this slice.
    #[error("quota refresh provider execution is not implemented in Plan 1A")]
    RefreshNotImplemented,
    /// Quota refresh provider request failed before a response status was available.
    #[error("quota refresh request failed: {message}")]
    ProviderRequest {
        /// Redacted request failure.
        message: String,
    },
    /// Quota refresh provider returned a non-success response.
    #[error("quota refresh provider returned HTTP {status}")]
    ProviderStatus {
        /// Provider HTTP status.
        status: u16,
    },
    /// Quota refresh provider response did not contain usable quota data.
    #[error("quota refresh provider response was unusable: {message}")]
    ProviderResponse {
        /// Redacted response failure.
        message: String,
    },
    /// Credential resolution failed before provider quota refresh.
    #[error(transparent)]
    CredentialResolver(#[from] CredentialResolverError),
    /// State-store operation failed.
    #[error(transparent)]
    StateStore(#[from] StateStoreError),
    /// Failed to initialize the serve-owned background refresh executor.
    #[error("failed to initialize quota history runtime: {0}")]
    BackgroundWorkerInitialization(std::io::Error),
    /// Stdout write failed.
    #[error("failed to write stdout: {0}")]
    Stdout(std::io::Error),
}
