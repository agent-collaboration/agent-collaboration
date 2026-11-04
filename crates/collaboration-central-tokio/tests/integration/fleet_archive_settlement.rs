//! A late finalization abort rolls back the real lease hook or claim receiver with archive state.
use super::{
    composed_fleet_fixture::*,
    fleet_archive_snapshot::{ArchiveState, inspect},
    fleet_claim_receiver::{claim_gate, claim_gate_call},
};
use collaboration_central_core::{ClaimReservationOutcome, ClaimReservationState, CoreHost};
use durable_steps::{
    AtomicStore, CallFailure, JournalScope, OperationId, StoreError,
    storage::{JournalStorage, OperationExecution},
};
use fleet_admission::FleetOperation;
use fleet_sync_lease::ImportTask;
use fleet_sync_protocol::{Import, ImportAnswer};
use fleet_sync_tokio_runner::TokioObject;
use message_board::{BoardState, FleetPosition, Record, RecordPlacement, WorkspaceState};
use runtime_primitives::Clock;
use serde_json::json;

type ImportAdmission =
    ImportTask<TokioObject<CoreHost<Io>>, sqlx_turso::Turso, fleet_sync_lease::NoLeaseChangeHook>;

async fn admit_import(fixture: &Fixture, request: Import) -> TestResult<OperationId> {
    let key = fleet_sync_lease::lifecycle_request_key(request.attempt)?;
    Ok(fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                // Only public kind/version/input are supplied. The registered native
                // Fleet engine executes this with its actual transactional lease hook.
                scope
                    .admit::<ImportAdmission>(&key, &request)
                    .await
                    .map(|record| record.id())
                    .map_err(StoreError::backend)
            })
        })
        .await?)
}

fn import_request() -> TestResult<Import> {
    Ok(Import {
        attempt: fleet_sync_protocol::RequestId::new(durable_steps::Uuid::now_v7())?,
        database: PROJECT.to_owned().try_into()?,
        digest: fleet_sync_protocol::ImportDigest::new("a".repeat(64))?,
    })
}

async fn require_imported(fixture: &Fixture, id: OperationId) -> TestResult {
    let result = fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                let record = scope
                    .operation(id)
                    .await?
                    .ok_or(StoreError::MissingOperation { id })?;
                let OperationExecution::Completed { result, .. } = record.execution else {
                    return Err(StoreError::backend("real Fleet Import did not complete"));
                };
                serde_json::from_slice::<ImportAnswer>(&result).map_err(StoreError::backend)
            })
        })
        .await?;
    if !matches!(result, ImportAnswer::Imported) {
        return Err(format!("unexpected Import result: {result:?}").into());
    }
    Ok(())
}

async fn archive(fixture: &Fixture) -> TestResult {
    let answer = fixture
        .metadata(FleetOperation::WorkspaceArchive {})
        .await?;
    if !matches!(
        serde_json::from_value::<fleet_admission::FleetRequestResult>(answer)?,
        fleet_admission::FleetRequestResult::Decided(
            message_board::RequestDecision::Accepted { .. }
        )
    ) {
        return Err("real workspace archive admission was refused".into());
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Finalization must produce exactly the two expected records and positioned feed entries without changing request decisions or reports."
)]
fn require_finalized(before: &ArchiveState, after: &ArchiveState, now: i64) -> TestResult {
    let mut position = FleetPosition::try_from(u64::try_from(
        before.feed.last().ok_or("archive feed absent")?.0,
    )?)?;
    let mut expected = before.records.clone();
    let at = chrono::DateTime::from_timestamp_millis(now)
        .ok_or("archive test clock outside UTC range")?;
    for record in &mut expected {
        match record {
            Record::Board(board) => {
                assert_eq!(board.state, BoardState::Archiving);
                board.state = BoardState::Archived;
            }
            Record::Workspace(workspace) => {
                assert_eq!(workspace.state, WorkspaceState::Archiving);
                workspace.state = WorkspaceState::Archived;
            }
            _ => return Err("unexpected archive record".into()),
        }
        position = position.successor()?;
        let envelope = record.envelope_mut();
        let RecordPlacement::Fleet(entry) = &mut envelope.position else {
            return Err("archive record lacks Fleet position".into());
        };
        entry.position = position;
        envelope.request = None;
        envelope.time = message_board::RecordTime::Router(at);
    }
    assert_eq!(after.records, expected);
    assert_eq!(after.feed.len(), before.feed.len() + 2);
    let (unchanged, appended) = after
        .feed
        .split_at_checked(before.feed.len())
        .ok_or("archive lost its existing feed prefix")?;
    assert_eq!(unchanged, before.feed);
    for ((actual_position, payload), record) in appended.iter().zip(&expected) {
        assert_eq!(
            *actual_position,
            match &record.envelope().position {
                RecordPlacement::Fleet(entry) => i64::try_from(entry.position.get())?,
                _ => return Err("expected Fleet position absent".into()),
            }
        );
        let entry = serde_json::from_str::<fleet_admission::FleetFeedEntry>(payload)?;
        assert_eq!(entry.position, position_for(record)?);
        assert_eq!(entry.record, *record);
    }
    assert_eq!(after.ledgers, before.ledgers);
    assert_eq!(after.reports, before.reports);
    assert_eq!(after.notices, before.notices);
    Ok(())
}

fn position_for(record: &Record) -> TestResult<FleetPosition> {
    record
        .envelope()
        .position
        .fleet_position()
        .ok_or_else(|| "archive record lacks Fleet position".into())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Actual Import reserve/hook finalization must roll back registry authority, directory, incarnation and archive effects after a late board failure, then commit once on the same durable retry."
)]
async fn genuine_unheld_hook_and_archive_completion_roll_back_then_retry_together() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    fixture.arrange_directory().await?;
    archive(&fixture).await?;
    let attempt = durable_steps::Uuid::now_v7();
    let before = inspect(&fixture, attempt).await?;
    assert!(before.projection.is_none());
    assert!(before.registry.is_none());
    assert!(before.incarnations.is_empty());
    let request = import_request()?;
    let operation = admit_import(&fixture, request.clone()).await?;
    fixture.sql("CREATE TRIGGER reject_archive_board BEFORE UPDATE ON boards BEGIN SELECT RAISE(ABORT,'final archive board fault'); END").await?;
    let report = fixture.runtime.on_alarm().await?;
    assert!(report.failed.is_empty(), "{report:?}");
    assert!(report.retried.contains(&operation), "{report:?}");
    assert_eq!(inspect(&fixture, attempt).await?, before);
    let retry_at = fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                let record = scope
                    .operation(operation)
                    .await?
                    .ok_or(StoreError::MissingOperation { id: operation })?;
                let OperationExecution::Running {
                    next_attempt_at, ..
                } = record.execution
                else {
                    return Err(StoreError::backend("aborted Import became terminal"));
                };
                Ok(next_attempt_at)
            })
        })
        .await?;
    fixture.sql("DROP TRIGGER reject_archive_board").await?;
    fixture
        .io
        .advance(retry_at.unix_millis() - fixture.io.now().unix_millis());
    let report = fixture.runtime.on_alarm().await?;
    assert!(report.failed.is_empty(), "{report:?}");
    require_imported(&fixture, operation).await?;
    let after = inspect(&fixture, attempt).await?;
    assert!(after.projection.is_some());
    assert!(
        matches!(after.registry.as_ref(),Some(snapshot) if matches!(snapshot.lifecycle,fleet_sync_lease::DatabaseLifecycle::Unheld {..})&&snapshot.grant.is_none())
    );
    assert_eq!(after.incarnations, vec![1]);
    assert!(after.reservation.is_none());
    require_finalized(&before, &after, fixture.io.now().unix_millis())?;
    assert_eq!(admit_import(&fixture, request).await?, operation);
    fixture.runtime.on_alarm().await?;
    assert_eq!(inspect(&fixture, attempt).await?, after);
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Cancelled claim settlement must roll back its reservation refusal with late archive completion, then replay exactly one committed result without changing the hook view."
)]
async fn refused_reservation_and_archive_completion_roll_back_then_retry_together() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    fixture.arrange_directory().await?;
    let operation = admit_import(&fixture, import_request()?).await?;
    fixture.runtime.on_alarm().await?;
    require_imported(&fixture, operation).await?;
    let attempt = durable_steps::Uuid::now_v7();
    assert_eq!(
        claim_gate(&fixture, durable_steps::Uuid::now_v7(), attempt, false).await?,
        json!({"kind":"reserved"})
    );
    archive(&fixture).await?;
    let before = inspect(&fixture, attempt).await?;
    assert_eq!(before.reservation, Some(ClaimReservationState::Active));
    let call = claim_gate_call(durable_steps::Uuid::now_v7(), attempt, true)?;
    fixture.sql("CREATE TRIGGER reject_archive_board BEFORE UPDATE ON boards BEGIN SELECT RAISE(ABORT,'final archive board fault'); END").await?;
    assert!(matches!(
        fixture.runtime.on_call(call.clone()).await,
        Err(CallFailure::AnswerLost)
    ));
    assert_eq!(inspect(&fixture, attempt).await?, before);
    fixture.sql("DROP TRIGGER reject_archive_board").await?;
    let reply = fixture.runtime.on_call(call.clone()).await?;
    let _: fleet_sync_protocol::SettlementRecorded = serde_json::from_slice(&reply)?;
    let after = inspect(&fixture, attempt).await?;
    assert_eq!(
        after.reservation,
        Some(ClaimReservationState::Settled {
            outcome: ClaimReservationOutcome::Refused
        })
    );
    assert_eq!(after.projection, before.projection);
    assert_eq!(after.registry, before.registry);
    assert_eq!(after.incarnations, before.incarnations);
    require_finalized(&before, &after, fixture.io.now().unix_millis())?;
    assert_eq!(fixture.runtime.on_call(call).await?, reply);
    assert_eq!(inspect(&fixture, attempt).await?, after);
    fixture.close().await
}
