//! Real local Schedule custody; seeded freeze rows stand in only for S7's producer.
use agent_automation::{
    ContinuityInput, ExecutionDestination, OperationId, ScheduleDefinition, ScheduleId, TimingRule,
};
use automation_storage::{
    AutomationStore, ScheduleCreate, ScheduleEdit, ScheduleMutation, StorageError,
    StoredAutomationOwner,
};
use collaboration_protocol::{EndpointRef, SessionRef};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
type ProofResult<Value = ()> = Result<Value, Box<dyn std::error::Error>>;
fn ensure(condition: bool, message: &str) -> ProofResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
async fn local_schedule(store: &mut AutomationStore) -> ProofResult<ScheduleId> {
    let instruction = store
        .create_instruction(
            &OperationId::generate(),
            &"Retained Schedule".to_owned().try_into()?,
            0,
        )
        .await?;
    Ok(store
        .create_schedule(&ScheduleCreate::<SessionRef, EndpointRef> {
            operation_id: OperationId::generate(),
            definition: ScheduleDefinition {
                instruction_id: instruction.instruction_id,
                timing: TimingRule::Interval { seconds: 600 },
                enabled: false,
                destination: ExecutionDestination::Unprepared,
                execution_timeout_seconds: None,
                model: Some("gpt-6-luna".into()),
                effort: Some("low".into()),
            },
            imported_continuity: ContinuityInput::None,
            now_ms: 0,
        })
        .await?
        .schedule_id)
}

#[tokio::test]
async fn schedule_owner_uses_stored_authority_and_retains_pending_after_reopen() -> ProofResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    ensure(
        store
            .schedule_command_owner(&ScheduleId::generate())
            .await?
            == StoredAutomationOwner::Central,
        "missing Schedule did not select central",
    )?;
    let id = local_schedule(&mut store).await?;
    ensure(
        store.schedule_command_owner(&id).await? == StoredAutomationOwner::Local,
        "pre-upgrade Schedule lost Local custody",
    )?;
    let mut sql =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    sqlx::query("INSERT INTO automation_handover(automation_id,state,watermark_local_ms,frozen_payload,digest) VALUES (?,'pending',0,?,?)")
        .bind(id.as_str()).bind(b"{}".as_slice()).bind("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a").execute(&mut sql).await?;
    store.close().await?;
    let mut store = AutomationStore::open(&path).await?;
    ensure(
        store.schedule_command_owner(&id).await? == StoredAutomationOwner::HandoverPending,
        "reopen lost pending Schedule custody",
    )?;
    sqlx::query("UPDATE automation_handover SET state='moved' WHERE automation_id=?")
        .bind(id.as_str())
        .execute(&mut sql)
        .await?;
    ensure(
        store.schedule_command_owner(&id).await? == StoredAutomationOwner::Central,
        "moved Schedule did not select central",
    )?;
    sqlx::query("UPDATE schedule_definitions SET authority='invalid' WHERE schedule_id=?")
        .bind(id.as_str())
        .execute(&mut sql)
        .await?;
    ensure(
        matches!(
            store.schedule_command_owner(&id).await,
            Err(StorageError::InvalidRecord)
        ),
        "unknown Schedule authority selected an owner",
    )?;
    sql.close().await?;
    store.close().await?;
    root.close()?;
    Ok(())
}

#[tokio::test]
async fn schedule_mutation_rechecks_freeze_before_receipt_replay_or_new_writes() -> ProofResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let id = local_schedule(&mut store).await?;
    let old_operation = OperationId::generate();
    let existing = ScheduleMutation::<SessionRef, EndpointRef> {
        operation_id: old_operation,
        schedule_id: id.clone(),
        edit: ScheduleEdit::SetEnabled { enabled: false },
        now_ms: 1,
    };
    store.mutate_schedule(&existing).await?;
    ensure(
        store.schedule_command_owner(&id).await? == StoredAutomationOwner::Local,
        "interleaving did not begin in Local custody",
    )?;
    let before = serde_json::to_value(
        store
            .inspect_schedule::<SessionRef, EndpointRef>(&id)
            .await?,
    )?;
    let mut sql =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    // Exact interleaving: local selection, external freeze commit, then owner transaction.
    sqlx::query("INSERT INTO automation_handover(automation_id,state,watermark_local_ms,frozen_payload,digest) VALUES (?,'pending',0,?,?)")
        .bind(id.as_str()).bind(b"{}".as_slice()).bind("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a").execute(&mut sql).await?;
    let version: i64 = sqlx::query_scalar("PRAGMA data_version")
        .fetch_one(&mut sql)
        .await?;
    ensure(
        matches!(
            store.mutate_schedule(&existing).await,
            Err(StorageError::AutomationHandoverPending)
        ),
        "freeze replayed a stale local success",
    )?;
    let new_operation = OperationId::generate();
    let attempt = ScheduleMutation::<SessionRef, EndpointRef> {
        operation_id: new_operation.clone(),
        schedule_id: id.clone(),
        edit: ScheduleEdit::SetEnabled { enabled: false },
        now_ms: 2,
    };
    ensure(
        matches!(
            store.mutate_schedule(&attempt).await,
            Err(StorageError::AutomationHandoverPending)
        ),
        "freeze accepted a new local mutation",
    )?;
    ensure(
        serde_json::to_value(
            store
                .inspect_schedule::<SessionRef, EndpointRef>(&id)
                .await?,
        )? == before,
        "frozen attempts changed the Schedule snapshot",
    )?;
    let receipts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM operation_receipts WHERE operation_id=?")
            .bind(new_operation.as_str())
            .fetch_one(&mut sql)
            .await?;
    ensure(
        receipts == 0,
        "refused local Schedule mutation wrote a receipt",
    )?;
    ensure(
        sqlx::query_scalar::<_, i64>("PRAGMA data_version")
            .fetch_one(&mut sql)
            .await?
            == version,
        "frozen attempts changed SQLite",
    )?;
    sql.close().await?;
    store.close().await?;
    root.close()?;
    Ok(())
}
