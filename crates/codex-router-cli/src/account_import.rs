//! OpenAI import construction and the shared auth-owned activation path.
use super::{
    AccountCommandError, AccountCredentialBundle, AccountId, AsyncSqliteStateStore,
    CredentialActivation, CredentialActivationError, CredentialActivationRequest, Provider,
    SecretStore,
};

/// OpenAI credential activation request used by account storage tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountImportRequest {
    account_id: AccountId,
    label: String,
    access_token: String,
    refresh_token: Option<String>,
    chatgpt_account_id: Option<String>,
}

impl AccountImportRequest {
    /// Creates an account import request.
    #[must_use]
    pub fn new(
        account_id: AccountId,
        label: impl Into<String>,
        access_token: impl Into<String>,
    ) -> Self {
        Self {
            account_id,
            label: label.into(),
            access_token: access_token.into(),
            refresh_token: None,
            chatgpt_account_id: None,
        }
    }

    /// Sets a required refresh token.
    #[must_use]
    pub fn with_refresh_token(mut self, refresh_token: impl Into<String>) -> Self {
        self.refresh_token = Some(refresh_token.into());
        self
    }

    /// Sets an optional refresh token.
    #[must_use]
    pub fn with_optional_refresh_token(mut self, refresh_token: Option<String>) -> Self {
        self.refresh_token = refresh_token;
        self
    }

    /// Sets the ChatGPT account id used by ChatGPT backend requests.
    #[must_use]
    pub fn with_chatgpt_account_id(mut self, chatgpt_account_id: impl Into<String>) -> Self {
        let chatgpt_account_id = chatgpt_account_id.into();
        if !chatgpt_account_id.trim().is_empty() {
            self.chatgpt_account_id = Some(chatgpt_account_id);
        }
        self
    }
}

/// Imports an already-parsed Codex OAuth auth record into router-owned SQLx state.
pub async fn import_codex_auth_from_request_async<S>(
    state: &AsyncSqliteStateStore,
    secrets: &S,
    request: AccountImportRequest,
) -> Result<(), AccountCommandError>
where
    S: SecretStore + Clone + Send + Sync + 'static,
{
    activate_account_import(state, secrets, prepare_account_import(request)).await
}

#[cfg(all(test, debug_assertions))]
pub(crate) async fn import_codex_auth_with_lock_observer_async<S>(
    state: &AsyncSqliteStateStore,
    secrets: &S,
    request: AccountImportRequest,
    observer: std::sync::mpsc::Sender<()>,
) -> Result<(), AccountCommandError>
where
    S: SecretStore + Clone + Send + Sync + 'static,
{
    let activation_request =
        prepare_account_import(request).with_lock_contention_observer_for_test(observer);
    activate_account_import(state, secrets, activation_request).await
}

fn prepare_account_import(request: AccountImportRequest) -> CredentialActivationRequest {
    let mut bundle =
        AccountCredentialBundle::imported_codex_auth(request.access_token, request.refresh_token);
    if let Some(chatgpt_account_id) = request.chatgpt_account_id {
        bundle = bundle.with_chatgpt_account_id(chatgpt_account_id);
    }
    CredentialActivationRequest::new(
        Provider::Openai,
        request.account_id,
        request.label,
        bundle.into(),
    )
}

async fn activate_account_import<S>(
    state: &AsyncSqliteStateStore,
    secrets: &S,
    activation_request: CredentialActivationRequest,
) -> Result<(), AccountCommandError>
where
    S: SecretStore + Clone + Send + Sync + 'static,
{
    CredentialActivation::activate_login(state, secrets, activation_request)
        .await
        .map_err(|error| match error {
            CredentialActivationError::AccountProviderMismatch => {
                AccountCommandError::AccountProviderMismatch
            }
            other => AccountCommandError::CredentialActivation(other),
        })?;

    Ok(())
}
