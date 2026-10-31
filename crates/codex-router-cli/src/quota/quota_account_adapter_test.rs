use super::*;
use agent_router_quota_tui::{AccountActionTarget, QuotaAccountLabel};
use codex_router_core::{ids::AccountId, provider::Provider};
use std::sync::Mutex;

fn captured_target() -> AccountActionTarget {
    AccountActionTarget {
        account_id: AccountId::new("captured").unwrap(),
        label: QuotaAccountLabel::new("captured").unwrap(),
        provider: Provider::Openai,
        position: 0,
    }
}

async fn seed_account(root: &std::path::Path, label: &str, status: AccountStatus) {
    let store = AsyncSqliteStateStore::open(&root.join("state.sqlite"))
        .await
        .unwrap();
    store
        .upsert_account(&AccountRecord::new(
            Provider::Openai,
            AccountId::new("captured").unwrap(),
            label,
            status,
        ))
        .await
        .unwrap();
    store.close().await.unwrap();
}

#[tokio::test]
async fn quota_account_port_retains_requires_disabled_from_the_real_removal_owner() {
    let root = tempfile::tempdir().unwrap();
    seed_account(root.path(), "captured", AccountStatus::Enabled).await;
    let commands = account_commands_with_clipboard(
        root.path().to_owned(),
        Arc::new(|_| panic!("remove cannot copy")),
    );
    let result = commands(QuotaAccountCommand {
        target: captured_target(),
        action: QuotaAccountAction::Remove,
    })
    .await;
    assert_eq!(
        result,
        QuotaAccountCommandResult::Refused {
            reason: QuotaAccountRefusal::RequiresDisabled
        }
    );
    let state = AsyncSqliteStateStore::open_read_only(&root.path().join("state.sqlite"))
        .await
        .unwrap();
    assert_eq!(state.list_accounts().await.unwrap().len(), 1);
    state.close().await.unwrap();
    assert!(
        !root.path().join("secrets").exists(),
        "refusal precedes credential access"
    );
}

#[tokio::test]
async fn quota_account_port_retains_the_captured_record_refusal_without_deleting() {
    let root = tempfile::tempdir().unwrap();
    seed_account(root.path(), "renamed", AccountStatus::Disabled).await;
    let commands = account_commands_with_clipboard(
        root.path().to_owned(),
        Arc::new(|_| panic!("remove cannot copy")),
    );
    assert_eq!(
        commands(QuotaAccountCommand {
            target: captured_target(),
            action: QuotaAccountAction::Remove
        })
        .await,
        QuotaAccountCommandResult::Refused {
            reason: QuotaAccountRefusal::TargetChanged
        }
    );
    let state = AsyncSqliteStateStore::open_read_only(&root.path().join("state.sqlite"))
        .await
        .unwrap();
    assert_eq!(state.list_accounts().await.unwrap().len(), 1);
    state.close().await.unwrap();
    assert!(!root.path().join("secrets").exists());
}

#[tokio::test]
async fn quota_account_port_changes_status_by_id_when_the_label_is_rebound() {
    let root = tempfile::tempdir().unwrap();
    seed_account(root.path(), "renamed", AccountStatus::Enabled).await;
    let state = AsyncSqliteStateStore::open(&root.path().join("state.sqlite"))
        .await
        .unwrap();
    let neighbor = AccountId::new("neighbor").unwrap();
    state
        .upsert_account(&AccountRecord::new(
            Provider::Openai,
            neighbor.clone(),
            "captured",
            AccountStatus::Enabled,
        ))
        .await
        .unwrap();
    let commands = account_commands_with_clipboard(
        root.path().to_owned(),
        Arc::new(|_| panic!("status cannot copy")),
    );
    for (action, status) in [
        (QuotaAccountAction::Disable, AccountStatus::Disabled),
        (QuotaAccountAction::Enable, AccountStatus::Enabled),
    ] {
        assert_eq!(
            commands(QuotaAccountCommand {
                target: captured_target(),
                action
            })
            .await,
            QuotaAccountCommandResult::Succeeded
        );
        assert_eq!(
            state
                .load_account(&captured_target().account_id)
                .await
                .unwrap()
                .unwrap()
                .status(),
            status
        );
        assert_eq!(
            state
                .load_account(&neighbor)
                .await
                .unwrap()
                .unwrap()
                .status(),
            AccountStatus::Enabled
        );
    }
    state.close().await.unwrap();
}

#[tokio::test]
async fn quota_account_port_copies_the_full_captured_id_through_the_terminal_writer() {
    let root = tempfile::tempdir().unwrap();
    seed_account(root.path(), "captured", AccountStatus::Enabled).await;
    let copied = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&copied);
    let commands = account_commands_with_clipboard(
        root.path().to_owned(),
        Arc::new(move |payload| {
            recorded.lock().unwrap().push(payload.to_owned());
            Ok(())
        }),
    );
    assert_eq!(
        commands(QuotaAccountCommand {
            target: captured_target(),
            action: QuotaAccountAction::CopyId
        })
        .await,
        QuotaAccountCommandResult::Succeeded
    );
    assert_eq!(*copied.lock().unwrap(), vec!["captured"]);
}
