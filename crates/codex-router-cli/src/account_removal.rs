//! Account removal coordinates the existing account writer and credential authority.

use std::io::Write;
use std::path::PathBuf;

use codex_router_secret_store::account_credential_lock::AccountCredentialLock;
use codex_router_state::account::{AccountRecord, AccountStatus};
use codex_router_state::sqlite::{
    AccountRemovalError, AsyncSqliteStateStore, AsyncWeeklyQuotaFloorMutationStore,
};

use super::{AccountCommandError, account_command_runtime};

pub(super) fn remove_account(
    stdout: &mut impl Write,
    router_root: PathBuf,
    account_label: String,
) -> Result<(), AccountCommandError> {
    let runtime = account_command_runtime()?;
    let account = runtime.block_on(async {
        let database_path = router_root.join("state.sqlite");
        let state = AsyncSqliteStateStore::open_read_only(&database_path).await?;
        let accounts = state.list_accounts().await;
        state.close().await?;
        let mut matches = accounts?
            .into_iter()
            .filter(|account| account.label() == account_label);
        let account =
            matches
                .next()
                .ok_or_else(|| AccountCommandError::RemovalAccountNotFound {
                    account_label: account_label.clone(),
                })?;
        if matches.next().is_some() {
            return Err(AccountCommandError::RemovalAccountAmbiguous {
                account_label: account_label.clone(),
            });
        }
        Ok::<_, AccountCommandError>(account)
    })?;
    runtime.block_on(remove_resolved_account(router_root, account))?;
    writeln!(stdout, "removed account: {account_label}").map_err(AccountCommandError::Stdout)
}

/// The CLI label resolver and quota's captured-ID adapter share the same removal authority.
pub(crate) async fn remove_resolved_account(
    router_root: PathBuf,
    account: AccountRecord,
) -> Result<(), AccountCommandError> {
    remove_resolved_account_inner(
        router_root,
        account,
        #[cfg(test)]
        None,
    )
    .await
}

#[cfg(test)]
pub(crate) type RemovalCommitHook = std::sync::Arc<
    dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync,
>;

#[cfg(test)]
pub(crate) async fn remove_resolved_account_with_commit_hook(
    router_root: PathBuf,
    account: AccountRecord,
    before_commit: RemovalCommitHook,
) -> Result<(), AccountCommandError> {
    remove_resolved_account_inner(router_root, account, Some(before_commit)).await
}

async fn remove_resolved_account_inner(
    router_root: PathBuf,
    account: AccountRecord,
    #[cfg(test)] before_commit: Option<RemovalCommitHook>,
) -> Result<(), AccountCommandError> {
    if account.status() != AccountStatus::Disabled {
        return Err(AccountRemovalError::RequiresDisabled {
            account_label: account.label().to_owned(),
        }
        .into());
    }
    let database_path = router_root.join("state.sqlite");
    // Match login/renewal lock ordering: account authority precedes the SQLite writer.
    let lock_account_id = account.account_id().clone();
    let lock_database_path = database_path.clone();
    let account_lock = tokio::task::spawn_blocking(move || {
        AccountCredentialLock::acquire(&lock_database_path, &lock_account_id)
    })
    .await
    .map_err(|_| AccountCommandError::CredentialLockUnavailable)?
    .map_err(|_| AccountCommandError::CredentialLockUnavailable)?;
    let mutation = AsyncWeeklyQuotaFloorMutationStore::open(&database_path).await?;
    let result = async {
        let pending = mutation.begin_disabled_account_removal(&account).await?;
        let secrets =
            crate::secret_store_factory::open_cli_secret_store_async(router_root.join("secrets"))
                .await
                .map_err(|_| AccountCommandError::RemovalCredentialCleanup)?;
        // Keep account authority in the blocking task until deletion really ends.
        let (account_lock, cleanup) = tokio::task::spawn_blocking(move || {
            let result =
                secrets.remove_account_credentials(account.provider(), account.account_id());
            (account_lock, result)
        })
        .await
        .map_err(|_| AccountCommandError::RemovalCredentialCleanup)?;
        cleanup.map_err(|_| AccountCommandError::RemovalCredentialCleanup)?;
        #[cfg(test)]
        if let Some(before_commit) = before_commit {
            before_commit().await;
        }
        pending.commit().await?;
        drop(account_lock);
        Ok::<(), AccountCommandError>(())
    }
    .await;
    mutation.close().await;
    result
}
