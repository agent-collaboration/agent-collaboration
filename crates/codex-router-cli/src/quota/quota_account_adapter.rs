//! Noninteractive quota actions reuse the captured-ID account owners.
use agent_router_quota_tui::{
    QuotaAccountAction, QuotaAccountCommand, QuotaAccountCommandResult, QuotaAccountCommands,
    QuotaAccountRefusal,
};
use codex_router_state::{
    account::{AccountRecord, AccountStatus},
    sqlite::{AsyncSqliteStateStore, AsyncWeeklyQuotaFloorMutationStore, StateStoreError},
};
use std::{io, path::PathBuf, sync::Arc};

type QuotaClipboardWriter = Arc<dyn Fn(&str) -> io::Result<()> + Send + Sync>;

pub(super) fn interactive_quota_account_commands(router_root: PathBuf) -> QuotaAccountCommands {
    account_commands_with_clipboard(
        router_root,
        Arc::new(|payload| {
            use crossterm::{ExecutableCommand, clipboard::CopyToClipboard};
            io::stdout().execute(CopyToClipboard::to_clipboard_from(payload))?;
            Ok(())
        }),
    )
}

fn account_commands_with_clipboard(
    router_root: PathBuf,
    clipboard: QuotaClipboardWriter,
) -> QuotaAccountCommands {
    Arc::new(move |request| {
        let router_root = router_root.clone();
        let clipboard = Arc::clone(&clipboard);
        Box::pin(execute_account_command(
            router_root,
            request,
            clipboard,
            #[cfg(test)]
            None,
        ))
    })
}

async fn execute_account_command(
    router_root: PathBuf,
    request: QuotaAccountCommand,
    clipboard: QuotaClipboardWriter,
    #[cfg(test)] before_commit: Option<crate::account::account_removal::RemovalCommitHook>,
) -> QuotaAccountCommandResult {
    let store = match AsyncSqliteStateStore::open_read_only(&router_root.join("state.sqlite")).await
    {
        Ok(store) => store,
        Err(error) => return failed(error.to_string()),
    };
    let current = store.load_account(&request.target.account_id).await;
    if let Err(error) = store.close().await {
        return failed(error.to_string());
    }
    let current = match current {
        Ok(Some(account)) => account,
        Ok(None) => {
            return QuotaAccountCommandResult::Refused {
                reason: QuotaAccountRefusal::AccountNoLongerListed,
            };
        }
        Err(error) => return failed(error.to_string()),
    };
    if current.provider() != request.target.provider {
        return QuotaAccountCommandResult::Refused {
            reason: QuotaAccountRefusal::TargetChanged,
        };
    }
    match request.action {
        QuotaAccountAction::CopyId => match clipboard(request.target.account_id.as_str()) {
            Ok(()) => QuotaAccountCommandResult::Succeeded,
            Err(error) => failed(format!("Copy account ID failed: {error}")),
        },
        QuotaAccountAction::Disable | QuotaAccountAction::Enable => {
            let writer =
                match AsyncWeeklyQuotaFloorMutationStore::open(&router_root.join("state.sqlite"))
                    .await
                {
                    Ok(writer) => writer,
                    Err(error) => return failed(error.to_string()),
                };
            let status = if request.action == QuotaAccountAction::Disable {
                AccountStatus::Disabled
            } else {
                AccountStatus::Enabled
            };
            let result = writer
                .set_account_status_by_account_id(&request.target.account_id, status)
                .await;
            writer.close().await;
            match result {
                Ok(()) => QuotaAccountCommandResult::Succeeded,
                Err(StateStoreError::AccountStatusAccountNotFound) => {
                    QuotaAccountCommandResult::Refused {
                        reason: QuotaAccountRefusal::AccountNoLongerListed,
                    }
                }
                Err(error) => failed(error.to_string()),
            }
        }
        QuotaAccountAction::Remove => {
            let mut expected = AccountRecord::new(
                request.target.provider,
                request.target.account_id,
                request.target.label.as_str(),
                current.status(),
            );
            if let Some(generation) = current.active_credential_generation() {
                expected = expected.with_active_credential_generation(generation);
            }
            #[cfg(test)]
            if let Some(before_commit) = before_commit {
                return account_command_result(
                    crate::account::account_removal::remove_resolved_account_with_commit_hook(
                        router_root,
                        expected,
                        before_commit,
                    )
                    .await,
                );
            }
            account_command_result(
                crate::account::account_removal::remove_resolved_account(router_root, expected)
                    .await,
            )
        }
    }
}

#[cfg(test)]
pub(crate) fn account_commands_with_commit_hook(
    router_root: PathBuf,
    before_commit: crate::account::account_removal::RemovalCommitHook,
) -> QuotaAccountCommands {
    Arc::new(move |request| {
        Box::pin(execute_account_command(
            router_root.clone(),
            request,
            Arc::new(|_| Ok(())),
            Some(before_commit.clone()),
        ))
    })
}

fn account_command_result(
    result: Result<(), crate::account::AccountCommandError>,
) -> QuotaAccountCommandResult {
    match result {
        Ok(()) => QuotaAccountCommandResult::Succeeded,
        Err(crate::account::AccountCommandError::RemovalState(error)) => {
            use codex_router_state::sqlite::AccountRemovalError;
            let reason = match error {
                AccountRemovalError::RequiresDisabled { .. } => {
                    QuotaAccountRefusal::RequiresDisabled
                }
                AccountRemovalError::TargetChanged => QuotaAccountRefusal::TargetChanged,
                AccountRemovalError::AccountNotFound => QuotaAccountRefusal::AccountNoLongerListed,
                other => return failed(other.to_string()),
            };
            QuotaAccountCommandResult::Refused { reason }
        }
        Err(error) => failed(error.to_string()),
    }
}

fn failed(message: String) -> QuotaAccountCommandResult {
    QuotaAccountCommandResult::Failed { message }
}

#[cfg(test)]
#[path = "quota_account_adapter_test.rs"]
mod tests;
