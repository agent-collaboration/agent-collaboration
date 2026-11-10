//! Router-owned provider credential resolution.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_router_core::ids::AccountId;
use codex_router_core::provider::Provider;
use codex_router_core::redaction::SecretString;
use codex_router_secret_store::SecretStore;
use codex_router_secret_store::account_tokens::AccountCredentialBundle;
use codex_router_secret_store::account_tokens::provider_credential_bundle_key;
use codex_router_secret_store::credential_bundle::CredentialBundle;
use codex_router_secret_store::model::SecretStoreError;
use codex_router_state::account::AccountStatus;
use codex_router_state::credential_maintenance::ClaimPurpose;
use codex_router_state::credential_maintenance::LOGIN_CREDENTIAL_CLAIM_TIMEOUT_SECONDS;
use codex_router_state::sqlite::AsyncSqliteStateStore;
use codex_router_state::sqlite::StateStoreError;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;
use tokio_util::task::TaskTracker;

use crate::openai_oauth::OPENAI_OAUTH_CLIENT_ID;
use crate::openai_oauth::OPENAI_OAUTH_TOKEN_ENDPOINT;
use codex_router_secret_store::account_credential_lock::AccountCredentialLock;
use codex_router_state::credential_maintenance::CredentialFailureClass;
use codex_router_state::credential_maintenance::CredentialMaintenanceRecord;
use codex_router_state::credential_maintenance::CredentialMaintenanceState;
use codex_router_state::credential_maintenance::CredentialRefreshClaimDisposition;

/// Credential resolver failure.
#[derive(Debug, Error)]
pub enum CredentialResolverError {
    /// Account metadata was unavailable or not usable.
    #[error("provider credential account is unavailable")]
    AccountUnavailable,
    /// Account is disabled or has no active credential generation.
    #[error("provider credential account is ineligible")]
    AccountIneligible,
    /// The selected account provider did not match persisted account identity.
    #[error("provider credential account does not match the expected provider")]
    AccountProviderMismatch,
    /// Secret material was unavailable or malformed.
    #[error("provider credential secret is unavailable")]
    SecretUnavailable,
    /// The process credential store could not provide pooled credentials.
    #[error("provider credential store is unavailable")]
    CredentialStoreUnavailable,
    /// Refresh is required but cannot be performed.
    #[error("provider credential refresh is unavailable")]
    RefreshUnavailable,
}

impl Clone for CredentialResolverError {
    fn clone(&self) -> Self {
        match self {
            Self::AccountUnavailable => Self::AccountUnavailable,
            Self::AccountIneligible => Self::AccountIneligible,
            Self::AccountProviderMismatch => Self::AccountProviderMismatch,
            Self::SecretUnavailable => Self::SecretUnavailable,
            Self::CredentialStoreUnavailable => Self::CredentialStoreUnavailable,
            Self::RefreshUnavailable => Self::RefreshUnavailable,
        }
    }
}

impl PartialEq for CredentialResolverError {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::AccountUnavailable, Self::AccountUnavailable)
                | (Self::AccountIneligible, Self::AccountIneligible)
                | (Self::AccountProviderMismatch, Self::AccountProviderMismatch)
                | (Self::SecretUnavailable, Self::SecretUnavailable)
                | (
                    Self::CredentialStoreUnavailable,
                    Self::CredentialStoreUnavailable
                )
                | (Self::RefreshUnavailable, Self::RefreshUnavailable)
        )
    }
}

impl Eq for CredentialResolverError {}

/// Resolved provider credential emitted immediately before upstream egress.
#[derive(Clone, Eq, PartialEq)]
pub struct ResolvedProviderCredential {
    account_id: AccountId,
    access_token: SecretString,
    chatgpt_account_id: Option<String>,
    credential_generation: u64,
}

impl ResolvedProviderCredential {
    /// Creates a resolved provider credential.
    #[must_use]
    pub const fn new(
        account_id: AccountId,
        access_token: SecretString,
        credential_generation: u64,
    ) -> Self {
        Self {
            account_id,
            access_token,
            chatgpt_account_id: None,
            credential_generation,
        }
    }

    /// Returns the selected account id.
    #[must_use]
    pub const fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    /// Returns the bearer access token.
    #[must_use]
    pub const fn access_token(&self) -> &SecretString {
        &self.access_token
    }

    /// Returns the ChatGPT account id for ChatGPT backend requests.
    #[must_use]
    pub fn chatgpt_account_id(&self) -> Option<&str> {
        self.chatgpt_account_id.as_deref()
    }

    /// Returns the credential generation that produced this access token.
    #[must_use]
    pub const fn credential_generation(&self) -> u64 {
        self.credential_generation
    }

    /// Sets the ChatGPT account id for ChatGPT backend requests.
    #[must_use]
    pub fn with_chatgpt_account_id(mut self, chatgpt_account_id: Option<&str>) -> Self {
        self.chatgpt_account_id = chatgpt_account_id.map(str::to_owned);
        self
    }
}

impl fmt::Debug for ResolvedProviderCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedProviderCredential")
            .field("account_id", &self.account_id)
            .field("access_token", &"<redacted>")
            .field("chatgpt_account_id", &self.chatgpt_account_id)
            .finish()
    }
}

/// Resolves provider credentials for selected accounts.
pub trait ProviderCredentialResolver {
    /// Resolves credentials immediately before provider egress.
    fn resolve_provider_credentials(
        &self,
        account_id: &AccountId,
        expected_provider: Provider,
    ) -> Result<ResolvedProviderCredential, CredentialResolverError>;
}

/// Refresh client used when an active access token is expired.
pub trait CredentialRefreshClient {
    /// Refreshes one account credential bundle.
    fn refresh_credentials(
        &self,
        account_id: &AccountId,
        refresh_token: &SecretString,
    ) -> Result<AccountCredentialBundle, CredentialRefreshFailure>;

    /// Refreshes the provider-specific bundle selected by the stored account identity.
    fn refresh_provider_credentials(
        &self,
        provider: Provider,
        account_id: &AccountId,
        refresh_token: &SecretString,
    ) -> Result<CredentialBundle, CredentialRefreshFailure> {
        if provider != Provider::Openai {
            return Err(CredentialRefreshFailure::confirmed_unspent(
                CredentialFailureClass::LocalPersistence,
                None,
            ));
        }
        self.refresh_credentials(account_id, refresh_token)
            .map(CredentialBundle::OpenAi)
    }
}

/// Secret-safe outcome of one OAuth refresh attempt.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("provider credential refresh failed ({failure_class:?})")]
pub struct CredentialRefreshFailure {
    pub failure_class: CredentialFailureClass,
    pub confirmed_unspent: bool,
    pub retry_after_seconds: Option<u64>,
}

impl CredentialRefreshFailure {
    /// The provider did not spend the refresh token and a retry is safe.
    #[must_use]
    pub const fn confirmed_unspent(
        failure_class: CredentialFailureClass,
        retry_after_seconds: Option<u64>,
    ) -> Self {
        Self {
            failure_class,
            confirmed_unspent: true,
            retry_after_seconds,
        }
    }

    /// The provider may have spent the refresh token; require device re-login.
    #[must_use]
    pub const fn ambiguous(failure_class: CredentialFailureClass) -> Self {
        Self {
            failure_class,
            confirmed_unspent: false,
            retry_after_seconds: None,
        }
    }
}

/// Refresh client used when runtime refresh is intentionally unavailable.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoopCredentialRefreshClient;

impl CredentialRefreshClient for NoopCredentialRefreshClient {
    fn refresh_credentials(
        &self,
        _account_id: &AccountId,
        _refresh_token: &SecretString,
    ) -> Result<AccountCredentialBundle, CredentialRefreshFailure> {
        Err(CredentialRefreshFailure::ambiguous(
            CredentialFailureClass::ProviderOutcomeAmbiguous,
        ))
    }
}

/// OpenAI OAuth refresh client compatible with Codex auth.json credentials.
#[derive(Clone, Debug)]
pub struct OpenAiOAuthRefreshClient {
    token_endpoint: String,
    client_id: String,
}

impl OpenAiOAuthRefreshClient {
    /// Creates a refresh client using Codex-compatible production defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            token_endpoint: OPENAI_OAUTH_TOKEN_ENDPOINT.to_owned(),
            client_id: OPENAI_OAUTH_CLIENT_ID.to_owned(),
        }
    }

    /// Creates a refresh client with explicit endpoint/client id for tests.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn new_with_endpoint(
        token_endpoint: impl Into<String>,
        client_id: impl Into<String>,
    ) -> Self {
        Self {
            token_endpoint: token_endpoint.into(),
            client_id: client_id.into(),
        }
    }
}

impl Default for OpenAiOAuthRefreshClient {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialRefreshClient for OpenAiOAuthRefreshClient {
    fn refresh_credentials(
        &self,
        _account_id: &AccountId,
        refresh_token: &SecretString,
    ) -> Result<AccountCredentialBundle, CredentialRefreshFailure> {
        self.refresh_with_token(refresh_token)
    }
}

/// Provider-selected refresh clients used by account resolution and upkeep.
#[derive(Clone, Debug)]
pub struct ProviderCredentialRefreshClients {
    openai: OpenAiOAuthRefreshClient,
    claude: crate::claude_oauth::ClaudeOAuthRefreshClient,
}

impl ProviderCredentialRefreshClients {
    /// Creates the Router-owned OpenAI and Claude subscription refresh clients.
    #[must_use]
    pub fn new() -> Self {
        Self {
            openai: OpenAiOAuthRefreshClient::new(),
            claude: crate::claude_oauth::ClaudeOAuthRefreshClient::new(),
        }
    }
}

impl Default for ProviderCredentialRefreshClients {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialRefreshClient for ProviderCredentialRefreshClients {
    fn refresh_credentials(
        &self,
        account_id: &AccountId,
        refresh_token: &SecretString,
    ) -> Result<AccountCredentialBundle, CredentialRefreshFailure> {
        self.openai.refresh_credentials(account_id, refresh_token)
    }

    fn refresh_provider_credentials(
        &self,
        provider: Provider,
        account_id: &AccountId,
        refresh_token: &SecretString,
    ) -> Result<CredentialBundle, CredentialRefreshFailure> {
        match provider {
            Provider::Openai => self
                .openai
                .refresh_credentials(account_id, refresh_token)
                .map(CredentialBundle::OpenAi),
            Provider::Claude => {
                self.claude
                    .refresh_provider_credentials(provider, account_id, refresh_token)
            }
        }
    }
}

#[derive(Clone, Copy)]
enum RenewalTrigger {
    ExpiredAccess,
    Proactive,
    UnauthorizedGeneration(u64),
}

impl OpenAiOAuthRefreshClient {
    fn refresh_with_token(
        &self,
        refresh_token: &SecretString,
    ) -> Result<AccountCredentialBundle, CredentialRefreshFailure> {
        let request = RefreshTokenRequest {
            client_id: &self.client_id,
            grant_type: "refresh_token",
            refresh_token: refresh_token.expose_secret(),
        };
        let body = serde_json::to_string(&request).map_err(|_| {
            CredentialRefreshFailure::confirmed_unspent(
                CredentialFailureClass::TransportUnspent,
                None,
            )
        })?;
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| {
                CredentialRefreshFailure::confirmed_unspent(
                    CredentialFailureClass::TransportUnspent,
                    None,
                )
            })?;
        let response = client
            .post(&self.token_endpoint)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .map_err(|error| {
                if error.is_connect() {
                    CredentialRefreshFailure::confirmed_unspent(
                        CredentialFailureClass::TransportUnspent,
                        None,
                    )
                } else {
                    CredentialRefreshFailure::ambiguous(
                        CredentialFailureClass::ProviderOutcomeAmbiguous,
                    )
                }
            })?;
        let status = response.status();
        let retry_after_seconds = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|header| header.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(CredentialRefreshFailure::confirmed_unspent(
                CredentialFailureClass::RateLimited,
                retry_after_seconds,
            ));
        }
        let body = response.text().map_err(|_| {
            CredentialRefreshFailure::ambiguous(CredentialFailureClass::ProviderOutcomeAmbiguous)
        })?;

        if !status.is_success() {
            let oauth_error = serde_json::from_str::<RefreshTokenErrorResponse>(&body)
                .ok()
                .and_then(|error| error.error);
            let failure = match (status.as_u16(), oauth_error.as_deref()) {
                (400, Some("invalid_grant")) => {
                    CredentialRefreshFailure::ambiguous(CredentialFailureClass::ProviderRejected)
                }
                (500..=599, Some("temporarily_unavailable")) => {
                    CredentialRefreshFailure::confirmed_unspent(
                        CredentialFailureClass::ProviderTemporary,
                        retry_after_seconds,
                    )
                }
                _ => CredentialRefreshFailure::ambiguous(
                    CredentialFailureClass::ProviderOutcomeAmbiguous,
                ),
            };
            return Err(failure);
        }

        let refresh_response =
            serde_json::from_str::<RefreshTokenResponse>(&body).map_err(|_| {
                CredentialRefreshFailure::ambiguous(CredentialFailureClass::MalformedResponse)
            })?;
        if refresh_response.access_token.trim().is_empty() {
            return Err(CredentialRefreshFailure::ambiguous(
                CredentialFailureClass::MalformedResponse,
            ));
        }
        if refresh_response
            .refresh_token
            .as_deref()
            .is_some_and(|replacement| replacement.trim().is_empty())
        {
            return Err(CredentialRefreshFailure::ambiguous(
                CredentialFailureClass::MalformedResponse,
            ));
        }
        let mut refreshed = AccountCredentialBundle::imported_codex_auth(
            refresh_response.access_token,
            Some(
                refresh_response
                    .refresh_token
                    .unwrap_or_else(|| refresh_token.expose_secret().to_owned()),
            ),
        );
        if let Some(expires_in_seconds) = refresh_response.expires_in {
            let expires_unix_seconds = current_unix_seconds()
                .unwrap_or(0)
                .saturating_add(expires_in_seconds);
            refreshed = refreshed.with_expires_unix_seconds(expires_unix_seconds);
        }

        Ok(refreshed)
    }
}

#[derive(Serialize)]
struct RefreshTokenRequest<'a> {
    client_id: &'a str,
    grant_type: &'a str,
    refresh_token: &'a str,
}

#[derive(Deserialize)]
struct RefreshTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct RefreshTokenErrorResponse {
    error: Option<String>,
}

/// Shared per-account async refresh leases for resolver single-flight behavior.
#[derive(Clone, Debug, Default)]
pub struct AsyncRefreshLeaseRegistry {
    leases: Arc<Mutex<HashMap<AccountId, Arc<tokio::sync::Mutex<()>>>>>,
}

impl AsyncRefreshLeaseRegistry {
    /// Creates an empty async refresh lease registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lease_for(&self, account_id: &AccountId) -> Arc<tokio::sync::Mutex<()>> {
        let mut leases = self
            .leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(
            leases
                .entry(account_id.clone())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }
}

fn map_state_error(_error: StateStoreError) -> CredentialResolverError {
    CredentialResolverError::AccountUnavailable
}

fn map_secret_error(error: SecretStoreError) -> CredentialResolverError {
    // Error Display/Debug may contain a credential path or a parser payload.
    // Keep the diagnostic useful without retaining either at this boundary.
    let failure_class = match &error {
        SecretStoreError::KeyUnavailable => "key_unavailable",
        SecretStoreError::KeyMissing => "key_missing",
        SecretStoreError::StoreUnavailable(_) => "store_unavailable",
        SecretStoreError::CredentialAuthenticationFailed => "envelope_authentication_failed",
        SecretStoreError::InvalidCredentialEnvelope => "invalid_envelope",
        SecretStoreError::InvalidCredentialEncoding => "invalid_encoding",
        SecretStoreError::InvalidSecretPayload { .. } => "invalid_payload",
        SecretStoreError::CredentialStoreLock { .. } => "store_lock_failed",
        SecretStoreError::SymlinkPath { .. } => "symlink_rejected",
        SecretStoreError::Filesystem { source, .. } => match source.kind() {
            std::io::ErrorKind::NotFound => "file_missing",
            std::io::ErrorKind::PermissionDenied => "file_permission_denied",
            _ => "filesystem_error",
        },
        _ => "unclassified",
    };
    tracing::warn!(
        credential_failure_class = failure_class,
        "provider credential local storage operation failed"
    );
    match error {
        SecretStoreError::KeyUnavailable
        | SecretStoreError::KeyMissing
        | SecretStoreError::StoreUnavailable(_) => {
            CredentialResolverError::CredentialStoreUnavailable
        }
        _ => CredentialResolverError::SecretUnavailable,
    }
}

mod credential_renewal;

pub use credential_renewal::AsyncRouterCredentialResolver;
pub use credential_renewal::CredentialRefreshTaskSupervisor;
pub use credential_renewal::DefaultAsyncRouterCredentialResolver;
#[cfg(test)]
pub(crate) use credential_renewal::credential_renewal_is_due;

/// Returns the current Unix second for runtime credential freshness checks.
pub fn current_unix_seconds() -> Result<u64, std::time::SystemTimeError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
