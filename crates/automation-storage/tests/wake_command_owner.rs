//! The command view reads real SQLite authority; seeded handover rows stand in for S7's producer.
use agent_automation::{ExpiryRule, OperationId, TimingRule, WakeupId};
use automation_storage::{AutomationStore, StorageError, StoredAutomationOwner, WakeCreate};
use collaboration_protocol::{MessageContent, MessageDelivery, SavedMessage, WakeCreator};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

type ProofResult<Value = ()> = Result<Value, Box<dyn std::error::Error>>;

fn ensure(condition: bool, message: &str) -> ProofResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

async fn local_wake(store: &mut AutomationStore) -> ProofResult<WakeupId> {
    let message = SavedMessage {
        target: serde_json::from_value(serde_json::json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
            "sessionId":"inert-local-wake"
        }))?,
        content: MessageContent::Router {
            text: "No native input".to_owned().try_into()?,
        },
        delivery: MessageDelivery::Queue,
        generation_guard: None,
    };
    Ok(store
        .create_wakeup(&WakeCreate {
            requested_by: WakeCreator::Router {},
            replace: false,
            operation_id: OperationId::generate(),
            message,
            timing: TimingRule::After { seconds: 60 },
            expiry: ExpiryRule::After { seconds: 3600 },
            now_ms: 0,
        })
        .await?
        .definition
        .wakeup_id)
}

#[tokio::test]
async fn local_and_missing_or_central_rows_choose_their_actual_owner() -> ProofResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    ensure(
        store.wake_command_owner(&WakeupId::generate()).await? == StoredAutomationOwner::Central,
        "missing Wake did not select central",
    )?;
    let wake = local_wake(&mut store).await?;
    ensure(
        store.wake_command_owner(&wake).await? == StoredAutomationOwner::Local,
        "pre-upgrade Wake lost Local custody",
    )?;
    let mut sql =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    sqlx::query("UPDATE wakeup_definitions SET authority='central' WHERE wakeup_id=?")
        .bind(wake.as_str())
        .execute(&mut sql)
        .await?;
    ensure(
        store.wake_command_owner(&wake).await? == StoredAutomationOwner::Central,
        "central projection selected Local",
    )?;
    sql.close().await?;
    store.close().await?;
    root.close()?;
    Ok(())
}

#[tokio::test]
async fn frozen_then_moved_custody_keeps_its_command_owner_after_reopen() -> ProofResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let wake = local_wake(&mut store).await?;
    let mut sql =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    // This seeds only S6's reader boundary. It is not evidence of a real S7 freeze/import.
    sqlx::query("INSERT INTO automation_handover(automation_id,state,watermark_local_ms,frozen_payload,digest) VALUES (?,'pending',0,?,?)")
        .bind(wake.as_str()).bind(b"{}".as_slice()).bind("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a").execute(&mut sql).await?;
    ensure(
        store.wake_command_owner(&wake).await? == StoredAutomationOwner::HandoverPending,
        "frozen Wake lost pending custody",
    )?;
    store.close().await?;
    let mut reopened = AutomationStore::open(&path).await?;
    ensure(
        reopened.wake_command_owner(&wake).await? == StoredAutomationOwner::HandoverPending,
        "reopen lost pending Wake custody",
    )?;
    sqlx::query("UPDATE automation_handover SET state='moved' WHERE automation_id=?")
        .bind(wake.as_str())
        .execute(&mut sql)
        .await?;
    ensure(
        reopened.wake_command_owner(&wake).await? == StoredAutomationOwner::Central,
        "moved Wake did not select central",
    )?;
    sql.close().await?;
    reopened.close().await?;
    root.close()?;
    Ok(())
}

#[tokio::test]
async fn unknown_authority_or_handover_state_is_refused_without_choosing_an_owner() -> ProofResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let wake = local_wake(&mut store).await?;
    let mut sql =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    sqlx::query("UPDATE wakeup_definitions SET authority='invalid' WHERE wakeup_id=?")
        .bind(wake.as_str())
        .execute(&mut sql)
        .await?;
    ensure(
        matches!(
            store.wake_command_owner(&wake).await,
            Err(StorageError::InvalidRecord)
        ),
        "unknown Wake authority selected an owner",
    )?;
    sqlx::query("UPDATE wakeup_definitions SET authority='local' WHERE wakeup_id=?")
        .bind(wake.as_str())
        .execute(&mut sql)
        .await?;
    sqlx::query("INSERT INTO automation_handover(automation_id,state,watermark_local_ms,frozen_payload,digest) VALUES (?,'invalid',0,?,?)")
        .bind(wake.as_str()).bind(b"{}".as_slice()).bind("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a").execute(&mut sql).await?;
    ensure(
        matches!(
            store.wake_command_owner(&wake).await,
            Err(StorageError::InvalidRecord)
        ),
        "unknown handover state selected an owner",
    )?;
    sql.close().await?;
    store.close().await?;
    root.close()?;
    Ok(())
}

#[tokio::test]
async fn a_local_selection_before_freeze_cannot_authorize_a_later_local_mutation() -> ProofResult {
    use automation_storage::{WakeAction, WakeMutation};
    let root = tempfile::tempdir()?;
    let path = root.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let wake = local_wake(&mut store).await?;
    // Arrange the actual interleaving: selection, then another writer freezes, then mutation.
    ensure(
        store.wake_command_owner(&wake).await? == StoredAutomationOwner::Local,
        "interleaving did not begin in Local custody",
    )?;
    let before = store.read_wakeup::<SavedMessage>(&wake).await?;
    let mut sql =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    sqlx::query("INSERT INTO automation_handover(automation_id,state,watermark_local_ms,frozen_payload,digest) VALUES (?,'pending',0,?,?)")
        .bind(wake.as_str()).bind(b"{}".as_slice()).bind("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a").execute(&mut sql).await?;
    let operation = OperationId::generate();
    let result = store
        .mutate_wakeup::<SavedMessage>(&WakeMutation {
            operation_id: operation.clone(),
            wakeup_id: wake.clone(),
            action: WakeAction::Pause,
            now_ms: 1,
        })
        .await;
    ensure(
        matches!(result, Err(StorageError::AutomationHandoverPending)),
        "a local mutation crossed frozen handover custody",
    )?;
    let after = store.read_wakeup::<SavedMessage>(&wake).await?;
    ensure(
        serde_json::to_value(after)? == serde_json::to_value(before)?,
        "refusal changed the local Wake",
    )?;
    let receipts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM operation_receipts WHERE operation_id=?")
            .bind(operation.as_str())
            .fetch_one(&mut sql)
            .await?;
    ensure(
        receipts == 0,
        "refused local mutation retained a committed receipt",
    )?;
    sql.close().await?;
    store.close().await?;
    root.close()?;
    Ok(())
}
