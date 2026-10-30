//! Account removal coordinates the existing account writer and credential authority.

use std::io::Write;
use std::path::PathBuf;

use codex_router_secret_store::account_credential_lock::AccountCredentialLock;
use codex_router_state::account::AccountStatus;
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
    runtime.block_on(async {
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
        if account.status() != AccountStatus::Disabled {
            return Err(AccountRemovalError::RequiresDisabled {
                account_label: account_label.clone(),
            }
            .into());
        }

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
            let secrets = crate::secret_store_factory::open_cli_secret_store_async(
                router_root.join("secrets"),
            )
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
            pending.commit().await?;
            drop(account_lock);
            Ok::<(), AccountCommandError>(())
        }
        .await;
        mutation.close().await;
        result
    })?;
    writeln!(stdout, "removed account: {account_label}").map_err(AccountCommandError::Stdout)
}
