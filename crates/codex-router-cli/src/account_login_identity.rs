//! Resolve login identity without replacing the stable ID of an existing account.
use super::{AccountCommandError, account_command_runtime};
use codex_router_core::{ids::AccountId, provider::Provider};
use codex_router_state::sqlite::AsyncSqliteStateStore;
use std::path::Path;

pub(super) fn resolve_login_account_id_at_router_root(
    router_root: &Path,
    label: &str,
    provider: Provider,
) -> Result<AccountId, AccountCommandError> {
    let state_database_path = router_root.join("state.sqlite");
    if !state_database_path.exists() {
        return account_id_from_label(label);
    }

    let runtime = account_command_runtime()?;
    let state = runtime.block_on(AsyncSqliteStateStore::open(&state_database_path))?;
    let account_id = resolve_login_account_id(&state, label, provider, &runtime);
    runtime.block_on(state.close())?;
    account_id
}

pub(super) fn resolve_login_account_id(
    state: &AsyncSqliteStateStore,
    label: &str,
    provider: Provider,
    runtime: &tokio::runtime::Runtime,
) -> Result<AccountId, AccountCommandError> {
    let accounts = runtime.block_on(state.list_accounts())?;
    if let Some(account) = accounts.iter().find(|account| account.label() == label) {
        return if account.provider() == provider {
            Ok(account.account_id().clone())
        } else {
            Err(AccountCommandError::DuplicateAccountLabel {
                label: label.to_owned(),
            })
        };
    }
    let account_id = account_id_from_label(label)?;
    if accounts
        .iter()
        .any(|account| account.account_id() == &account_id)
    {
        return Err(AccountCommandError::DuplicateAccountLabel {
            label: label.to_owned(),
        });
    }
    Ok(account_id)
}

pub(super) fn normalize_label(label: &str) -> Result<String, AccountCommandError> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return Err(AccountCommandError::EmptyLabel);
    }

    Ok(trimmed.to_owned())
}

pub(super) fn account_id_from_label(label: &str) -> Result<AccountId, AccountCommandError> {
    let mut normalized = String::new();
    let mut previous_was_separator = false;
    for character in label.chars() {
        if character.is_ascii_alphanumeric() {
            normalized.extend(character.to_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator {
            normalized.push('_');
            previous_was_separator = true;
        }
    }
    let normalized = normalized.trim_matches('_');
    let stem = if normalized.is_empty() {
        "imported"
    } else {
        normalized
    };

    AccountId::new(format!("acct_{stem}")).map_err(|_| AccountCommandError::EmptyLabel)
}
