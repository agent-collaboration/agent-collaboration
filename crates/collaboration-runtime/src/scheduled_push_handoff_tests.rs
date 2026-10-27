//! Real worker outcomes remain owed until the native owner commits and automation acknowledges.
use super::route_tests::{
    FakeSettlementPlan, FakeSubmissionPlan, TestResult, provider_worker_fixture,
};
use agent_automation::RunPhase;
use collaboration_protocol::{
    CodexGeneration, DeliveryOutcome, EndpointRef, PushDeliveryState, SessionRef,
};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use sqlx_turso::{TursoConnectOptions, TursoConnection};
use std::{path::Path, sync::Arc};

async fn automation_observer(root: &Path) -> TestResult<SqliteConnection> {
    Ok(SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(root.join("automation.sqlite")),
    )
    .await?)
}

async fn close_fixture(root: &Path, worker: super::ScheduledRunWorker) -> TestResult<()> {
    let automation = Arc::clone(&worker.store);
    let interactions = Arc::clone(&worker.interactions);
    drop(worker);
    Arc::try_unwrap(interactions)
        .map_err(|_| "handoff fixture interaction owner remained shared")?
        .close()
        .await?;
    Arc::try_unwrap(automation)
        .map_err(|_| "handoff fixture automation owner remained shared")?
        .into_inner()
        .close()
        .await?;
    for entry in std::fs::read_dir(root)? {
        std::fs::remove_file(entry?.path())?;
    }
    std::fs::remove_dir(root)?;
    Ok(())
}

#[tokio::test]
async fn committed_schedule_outcome_replays_lost_ack_before_another_native_attempt()
-> TestResult<()> {
    let (root, automation, worker, run_id, attempts) = provider_worker_fixture(
        FakeSubmissionPlan::RejectOnceThenAccept,
        FakeSettlementPlan::Pending,
    )
    .await?;
    for _ in 0..3 {
        worker.step(run_id.clone()).await?;
    }
    let owed = automation
        .lock()
        .await
        .list_owed_push_operations(100)
        .await?;
    let [operation] = owed.as_slice() else {
        return Err("one run submission must commit exactly one owed settlement".into());
    };
    let push_id = operation.push_id.clone();
    let run = automation
        .lock()
        .await
        .read_run::<SessionRef, EndpointRef, CodexGeneration, crate::stored_run_receipt::StoredRunReceipt>(&run_id)
        .await?;
    let attempted = worker
        .interactions
        .get_push_record(&push_id)
        .await?
        .ok_or("attempted schedule push missing")?;
    if run.phase != RunPhase::Preparing
        || run.evidence.timing.is_some()
        || attempted.delivery_state != PushDeliveryState::Attempted
        || attempted.last_outcome.is_some()
    {
        return Err("committed run must precede its separately owed push settlement".into());
    }
    worker.step(run_id.clone()).await?;
    if attempts
        .lock()
        .map_err(|_| "attempt witness poisoned")?
        .len()
        != 2
    {
        return Err("worker submitted again before the owed rejection was applied".into());
    }

    let mut observer = automation_observer(&root).await?;
    sqlx::raw_sql("CREATE TRIGGER retain_owed_ack BEFORE DELETE ON owed_push_operations BEGIN SELECT RAISE(ABORT,'lost acknowledgement'); END")
        .execute(&mut observer).await?;
    let mut handoff =
        crate::PushHandoffWorker::new(Arc::clone(&automation), Arc::clone(&worker.interactions));
    if !matches!(
        handoff.apply_pending().await,
        Err(crate::PushHandoffError::Acknowledge { .. })
    ) {
        return Err("fixture did not fail after the native settlement commit".into());
    }
    let applied = worker
        .interactions
        .get_push_record(&push_id)
        .await?
        .ok_or("applied schedule push missing")?;
    if applied.delivery_state != PushDeliveryState::Pending
        || !matches!(
            applied
                .last_outcome
                .as_ref()
                .map(|receipt| &receipt.outcome),
            Some(DeliveryOutcome::Rejected(_))
        )
        || serde_json::to_value(
            automation
                .lock()
                .await
                .list_owed_push_operations(100)
                .await?,
        )? != serde_json::to_value(&owed)?
    {
        return Err(
            "lost acknowledgement must retain both the native result and exact owed input".into(),
        );
    }
    worker.step(run_id.clone()).await?;
    if attempts
        .lock()
        .map_err(|_| "attempt witness poisoned")?
        .len()
        != 2
    {
        return Err(
            "worker retried while a committed rejection still awaited acknowledgement".into(),
        );
    }

    let mut native = TursoConnection::connect_with(
        &TursoConnectOptions::new().filename(root.join("interaction.sqlite")),
    )
    .await?;
    sqlx::raw_sql("CREATE TRIGGER reject_replayed_push BEFORE UPDATE ON router_pushes BEGIN SELECT RAISE(ABORT,'replay rewrote native push'); END")
        .execute(&mut native).await?;
    sqlx::raw_sql("DROP TRIGGER retain_owed_ack")
        .execute(&mut observer)
        .await?;
    if handoff.apply_pending().await? != 1
        || serde_json::to_value(worker.interactions.get_push_record(&push_id).await?)?
            != serde_json::to_value(Some(applied))?
        || automation
            .lock()
            .await
            .has_owed_push_operations(&push_id)
            .await?
    {
        return Err(
            "replay must acknowledge exactly once without rewriting its applied result".into(),
        );
    }
    sqlx::raw_sql("DROP TRIGGER reject_replayed_push")
        .execute(&mut native)
        .await?;
    native.close().await?;
    observer.close().await?;
    worker.step(run_id.clone()).await?;
    if attempts
        .lock()
        .map_err(|_| "attempt witness poisoned")?
        .len()
        != 3
    {
        return Err(
            "acknowledgement did not release the original push for its known-safe retry".into(),
        );
    }
    if handoff.apply_pending().await? != 1
        || worker
            .interactions
            .get_push_record(&push_id)
            .await?
            .is_none_or(|push| push.delivery_state != PushDeliveryState::Delivered)
    {
        return Err("accepted retry did not settle the same schedule push".into());
    }
    drop(handoff);
    drop(automation);
    close_fixture(&root, worker).await
}

#[tokio::test]
async fn wrong_native_schedule_state_refuses_handoff_and_keeps_the_owed_input() -> TestResult<()> {
    let (root, automation, worker, run_id, attempts) = provider_worker_fixture(
        FakeSubmissionPlan::RejectOnceThenAccept,
        FakeSettlementPlan::Pending,
    )
    .await?;
    for _ in 0..3 {
        worker.step(run_id.clone()).await?;
    }
    let owed = automation
        .lock()
        .await
        .list_owed_push_operations(100)
        .await?;
    let [operation] = owed.as_slice() else {
        return Err("one owed settlement expected".into());
    };
    let push_id = operation.push_id.clone();
    let pending = worker
        .interactions
        .restore_schedule_push_pending(&push_id)
        .await?;
    let mut handoff =
        crate::PushHandoffWorker::new(Arc::clone(&automation), Arc::clone(&worker.interactions));
    match handoff.apply_pending().await {
        Err(crate::PushHandoffError::Apply { source, .. })
            if matches!(
                *source,
                interaction_storage::StorageError::PushStateConflict
            ) => {}
        _ => return Err("handoff accepted a different native state".into()),
    }
    if serde_json::to_value(
        automation
            .lock()
            .await
            .list_owed_push_operations(100)
            .await?,
    )? != serde_json::to_value(owed)?
        || serde_json::to_value(worker.interactions.get_push_record(&push_id).await?)?
            != serde_json::to_value(Some(pending))?
    {
        return Err("refused native apply changed the push or discarded its owed operation".into());
    }
    worker.step(run_id).await?;
    if attempts
        .lock()
        .map_err(|_| "attempt witness poisoned")?
        .len()
        != 2
    {
        return Err("unapplied settlement allowed another native attempt".into());
    }
    drop(handoff);
    drop(automation);
    close_fixture(&root, worker).await
}
