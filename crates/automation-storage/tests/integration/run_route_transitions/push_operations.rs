//! The run outcome and its owed notice share the same real SQLite commit.
use super::*;
use automation_storage::{PushOperation, ScheduleRunPushUpdate};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, PushDeliveryState, RouterOriginRef,
};
use sqlx::{SqliteConnection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn run_outcome_and_owed_push_settlement_commit_and_roll_back_together()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::temp_dir().join(format!(
        "run-push-atomic-{}",
        OperationId::generate().as_str()
    ));
    std::fs::create_dir(&directory)?;
    let path = directory.join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let run_id = admitted_run(&mut store).await?;
    let attempt = AttemptId::generate();
    store
        .begin_run_preparation::<String, String, String, String>(RunPreparationIntent {
            run_id: run_id.clone(),
            effects: provider_evidence(
                attempt.clone(),
                SubmissionEffect::NotDispatched,
                ProviderSettlementEffect::NotObserved,
            )?,
        })
        .await?;
    store
        .begin_run_dispatch::<String, String, String, String>(RunDispatchIntent {
            run_id: run_id.clone(),
            effects: provider_evidence(
                attempt.clone(),
                SubmissionEffect::Dispatching,
                ProviderSettlementEffect::NotObserved,
            )?,
            configured_timeout_seconds: 120,
            now_ms: 62_000,
        })
        .await?;
    let before = store
        .read_run::<String, String, String, String>(&run_id)
        .await?;
    let push_id = uuid::Uuid::now_v7().to_string().try_into()?;
    let settled_at = chrono::DateTime::from_timestamp_millis(63_000).ok_or("settlement time")?;
    let request = RunSubmissionResult {
        run_id: run_id.clone(),
        effects: provider_evidence(
            attempt,
            SubmissionEffect::Accepted,
            ProviderSettlementEffect::NotObserved,
        )?,
        outcome: RunSubmissionOutcome::ProviderAdmitted {
            receipt: "provider-operation".to_owned(),
        },
    };
    let update = ScheduleRunPushUpdate {
        push_id,
        origin_router_ref: RouterOriginRef::ScheduleRun {
            schedule_id: before.schedule_id.clone(),
            run_id: run_id.clone(),
        },
        delivery_state: PushDeliveryState::Delivered,
        receipt: DeliveryReceipt {
            outcome: DeliveryOutcome::Started,
            reachability: None,
            client: None,
        },
        settled_at,
    };
    let mut observer =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path)).await?;
    sqlx::raw_sql("CREATE TRIGGER reject_run_outcome BEFORE UPDATE OF run_status ON workflow_runs WHEN NEW.run_status='executing' BEGIN SELECT RAISE(ABORT,'run outcome fault after owed insertion'); END")
        .execute(&mut observer).await?;
    if store
        .record_run_submission_with_push::<String, String, String, String>(
            request.clone(),
            update.clone(),
        )
        .await
        .is_ok()
    {
        return Err("run fault did not reach the actual submission transaction".into());
    }
    if !store.list_owed_push_operations(100).await?.is_empty()
        || serde_json::to_value(
            store
                .read_run::<String, String, String, String>(&run_id)
                .await?,
        )? != serde_json::to_value(before)?
    {
        return Err("failed run outcome leaked its owed settlement or changed the run".into());
    }
    sqlx::raw_sql("DROP TRIGGER reject_run_outcome")
        .execute(&mut observer)
        .await?;
    observer.close().await?;
    store
        .record_run_submission_with_push::<String, String, String, String>(request, update.clone())
        .await?;
    store.close().await?;

    let mut store = AutomationStore::open(&path).await?;
    let run = store
        .read_run::<String, String, String, String>(&run_id)
        .await?;
    let owed = store.list_owed_push_operations(100).await?;
    let [operation] = owed.as_slice() else {
        return Err(
            "one committed run must retain exactly one owed settlement after reopen".into(),
        );
    };
    let PushOperation::SettleScheduleRun {
        push_id,
        origin_ref,
        state,
        receipt,
        settled_at: stored_at,
    } = &operation.operation
    else {
        return Err("wrong owed operation for a run submission".into());
    };
    if run.phase != RunPhase::Executing
        || run.evidence.acceptance.as_deref() != Some("provider-operation")
        || push_id != &update.push_id
        || origin_ref != &update.origin_router_ref
        || *state != PushDeliveryState::Delivered
        || *stored_at != settled_at
        || serde_json::to_value(receipt)? != serde_json::to_value(&update.receipt)?
    {
        return Err("cold reopen did not retain the exact run outcome and its owed notice".into());
    }
    store.close().await?;
    for entry in std::fs::read_dir(&directory)? {
        std::fs::remove_file(entry?.path())?;
    }
    std::fs::remove_dir(directory)?;
    Ok(())
}
