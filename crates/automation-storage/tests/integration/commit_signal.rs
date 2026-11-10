//! Q2 at the automation store: committed writes that create work wake subscribers;
//! reads and writes that change nothing don't.
use agent_automation::{ExpiryRule, OperationId, TimingRule};
use automation_storage::{AutomationStore, RemoteWakeMail, WakeCreate};
use collaboration_protocol::SavedMessage;
use serde_json::json;
use tokio::sync::watch;

/// A closed signal reads as "no bump", so a dropped store can only fail a "bumped" check.
fn has_bumped(commits: &mut watch::Receiver<u64>) -> bool {
    let changed = commits.has_changed().unwrap_or(false);
    commits.borrow_and_update();
    changed
}

#[expect(
    clippy::unwrap_used,
    reason = "Invalid test fixtures must fail the test immediately."
)]
fn saved_message() -> SavedMessage {
    serde_json::from_value(json!({
        "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000002","endpointId":"codex-local"},"sessionId":"recipient"},
        "content":{"kind":"humanUser","text":"saved"},"delivery":"auto"
    }))
    .unwrap()
}

#[expect(
    clippy::unwrap_used,
    reason = "Invalid test fixtures must fail the test immediately."
)]
fn remote_wake(message: &SavedMessage) -> RemoteWakeMail {
    let operation_id = OperationId::generate();
    RemoteWakeMail {
        operation_id: operation_id.clone(),
        target: message.target.clone(),
        canonical_request: vec![1, 2, 3],
        body: serde_json::from_value(json!({"requestedBy":{"kind":"owner"},"request":{"kind":"send","operationId":operation_id,"replace":false,"message":message,"timing":{"kind":"after","seconds":60},"expiry":{"kind":"none"}}})).unwrap(),
        valid_until_ms: 10_000,
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_created_wake_bumps_and_the_worker_reads_after_it_do_not()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let mut store = AutomationStore::open(&directory.path().join("automation.sqlite")).await?;
    let mut commits = store.subscribe_commits();

    let created = store
        .create_wakeup(&WakeCreate {
            requested_by: collaboration_protocol::WakeCreator::Owner {},
            replace: false,
            operation_id: OperationId::generate(),
            message: saved_message(),
            timing: TimingRule::After { seconds: 60 },
            expiry: ExpiryRule::None,
            now_ms: 1_000,
        })
        .await?;
    let created_bumped = has_bumped(&mut commits);
    store
        .read_wakeup::<SavedMessage>(&created.definition.wakeup_id)
        .await?;
    store.list_owed_push_operations(16).await?;
    store.eligible_delivery_ids(1_000_000, 16).await?;
    store.due_wakeup_ids(1_000, 16).await?;
    let reads_bumped = has_bumped(&mut commits);

    assert!(
        created_bumped,
        "a committed wake creation must wake its worker"
    );
    assert!(
        !reads_bumped,
        "the worker's inventory reads must not wake anyone"
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_former_single_statement_write_bumps_only_when_it_changes_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let mut store = AutomationStore::open(&directory.path().join("automation.sqlite")).await?;
    let mut commits = store.subscribe_commits();
    let request = remote_wake(&saved_message());

    store.prepare_remote_wake(request.clone()).await?;
    let insert_bumped = has_bumped(&mut commits);
    store.prepare_remote_wake(request).await?;
    let replay_bumped = has_bumped(&mut commits);
    let pruned = store.prune_terminal_wakeups(4_000_000_000_000, 16).await?;
    let empty_prune_bumped = has_bumped(&mut commits);

    assert!(insert_bumped, "the one-statement insert must signal");
    assert!(!replay_bumped, "a replay reads the frozen request only");
    assert_eq!(pruned, 0);
    assert!(
        !empty_prune_bumped,
        "a delete that removed nothing must not signal"
    );
    Ok(())
}
