//! A late archive write failure rolls back the mutation that removed its blocker.
use super::{
    claim_admission,
    claim_settlement::{acquire_incarnation, registry_call},
    metadata::request,
    support::{Fixture, TestResult, bundle},
};
use collaboration_central_core::{ClaimReservationOutcome, ClaimReservationState};
use durable_steps::{AtomicStore, CallFailure, CallTarget, StoreError, storage::TransactionScope};
use fleet_admission::FleetOperation;
use fleet_sync_lease::{DatabaseLifecycle, RegistryRevision, RegistrySnapshot};
use fleet_sync_protocol::{DatabaseId, Epoch, ImportDigest, SettleOutcome};
use message_board::{BoardState, FleetPosition, Record, RecordPlacement, WorkspaceState};
use runtime_primitives::SyncTime;
use serde_json::{Value, json};
use sqlx_core::{executor::Executor, query::query, row::Row};
use std::num::NonZeroU64;

const PROJECT: &str = "019f0000-0000-7000-8000-000000000a83";
const ATTEMPT: &str = "019f0000-0000-7000-8000-000000000b01";

#[derive(Debug, PartialEq)]
struct ArchiveState {
    records: Vec<Record>,
    feed: Vec<(i64, String)>,
    ledgers: Vec<String>,
    reports: Vec<String>,
    projection: Option<String>,
    reservation: Option<ClaimReservationState>,
}
async fn inspect(fixture: &Fixture) -> TestResult<ArchiveState> {
    Ok(fixture.object.transaction(|scope|Box::pin(async move {
        let rows = query::<sqlx_turso::Turso>("SELECT 'board' AS kind,payload FROM boards UNION ALL SELECT 'workspace' AS kind,payload FROM workspaces ORDER BY kind").fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
        let records = rows.into_iter().map(|row|{
            let payload=row.try_get::<String,_>("payload").map_err(StoreError::backend)?;
            serde_json::from_str(&payload).map_err(StoreError::backend)
        }).collect::<Result<Vec<_>,_>>()?;
        let feed = query::<sqlx_turso::Turso>("SELECT position,payload FROM fleet_feed ORDER BY position").fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter().map(|row|Ok((row.try_get::<i64,_>("position").map_err(StoreError::backend)?,row.try_get::<String,_>("payload").map_err(StoreError::backend)?))).collect::<Result<Vec<_>,StoreError>>()?;
        let ledgers = query::<sqlx_turso::Turso>("SELECT payload FROM fleet_request_decisions ORDER BY request_id").fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter().map(|row|row.try_get::<String,_>("payload").map_err(StoreError::backend)).collect::<Result<Vec<_>,_>>()?;
        let reports = query::<sqlx_turso::Turso>("SELECT report FROM archive_reports ORDER BY board_id,project_id").fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter().map(|row|row.try_get::<String,_>("report").map_err(StoreError::backend)).collect::<Result<Vec<_>,_>>()?;
        let row = query::<sqlx_turso::Turso>("SELECT lease_projection,(SELECT state FROM claim_reservations WHERE attempt_id=?) AS reservation FROM project_directory WHERE project_id=?").bind(ATTEMPT).bind(PROJECT).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        let reservation = row.try_get::<Option<String>,_>("reservation").map_err(StoreError::backend)?.map(|payload|serde_json::from_str(&payload).map_err(StoreError::backend)).transpose()?;
        Ok(ArchiveState { records, feed, ledgers, reports, projection:row.try_get("lease_projection").map_err(StoreError::backend)?,reservation })
    })).await?)
}
async fn board_write_fault(fixture: &Fixture, enabled: bool) -> TestResult {
    fixture.object.transaction(move |scope|Box::pin(async move {
        let sql = if enabled {
            "CREATE TRIGGER reject_archive_board BEFORE UPDATE ON boards BEGIN SELECT RAISE(ABORT,'final archive board fault'); END"
        } else { "DROP TRIGGER reject_archive_board" };
        scope.executor().execute(sql).await.map_err(StoreError::backend)?;
        Ok(())
    })).await?;
    Ok(())
}
async fn unheld_call(fixture: &Fixture) -> TestResult<Vec<u8>> {
    let incarnation = acquire_incarnation(fixture).await?;
    let snapshot = RegistrySnapshot {
        database: DatabaseId::new(durable_steps::Uuid::parse_str(PROJECT)?)?,
        revision: RegistryRevision {
            epoch: Epoch::new(1)?,
            change: NonZeroU64::new(1).ok_or("positive fixture change")?,
        },
        changed_at: SyncTime::from_unix_millis(1_792_281_600_000),
        lifecycle: DatabaseLifecycle::Unheld {
            import_digest: ImportDigest::new("a".repeat(64))?,
        },
        grant: None,
    };
    let holder: Option<message_board::ServiceId> = None;
    let bytes = serde_json::to_vec(&(&snapshot, &holder))?;
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    registry_call(
        "central.lease-projection",
        json!({"snapshot":snapshot,"holder":holder,"incarnation":incarnation,"digest":digest}),
        durable_steps::Uuid::now_v7(),
    )
}
fn assert_finalized(before: &ArchiveState, after: &ArchiveState) -> TestResult {
    let mut position = FleetPosition::try_from(u64::try_from(
        before.feed.last().ok_or("archive feed absent")?.0,
    )?)?;
    let mut expected = before.records.clone();
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
            return Err("archive placement absent".into());
        };
        entry.position = position;
        envelope.request = None;
        envelope.time = message_board::RecordTime::Router("2026-10-18T00:00:00Z".parse()?);
    }
    assert_eq!(after.records, expected);
    assert_eq!(
        after.feed.len(),
        before
            .feed
            .len()
            .checked_add(2)
            .ok_or("feed count overflow")?
    );
    assert_eq!(after.ledgers, before.ledgers);
    Ok(())
}

#[tokio::test]
async fn refused_reservation_and_archive_completion_roll_back_and_retry_together() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        fixture.import(&bundle()?).await?;
        assert_eq!(
            claim_admission::reserve(&fixture, durable_steps::Uuid::now_v7()).await?,
            fleet_sync_protocol::GateAnswer::Reserved
        );
        let projection = unheld_call(&fixture).await?;
        fixture
            .runtime
            .on_call(projection)
            .await
            .map_err(|error| format!("Unheld setup: {error:?}"))?;
        fixture
            .fleet_request(&request(FleetOperation::WorkspaceArchive {})?)
            .await?;
        let before = inspect(&fixture).await?;
        assert_eq!(before.reservation, Some(ClaimReservationState::Active));
        let cancel = registry_call(
            fleet_sync_lease::ClaimSettlement::KIND,
            serde_json::to_value(fleet_sync_protocol::ClaimSettlement {
                change: claim_admission::change()?,
                outcome: SettleOutcome::Cancelled,
            })?,
            durable_steps::Uuid::now_v7(),
        )?;
        board_write_fault(&fixture, true).await?;
        assert!(matches!(
            fixture.runtime.on_call(cancel.clone()).await,
            Err(CallFailure::AnswerLost)
        ));
        assert_eq!(inspect(&fixture).await?, before);
        board_write_fault(&fixture, false).await?;
        let bytes = fixture
            .runtime
            .on_call(cancel.clone())
            .await
            .map_err(|error| format!("cancel retry: {error:?}"))?;
        let _: fleet_sync_protocol::SettlementRecorded = serde_json::from_slice(&bytes)?;
        let after = inspect(&fixture).await?;
        assert_eq!(
            after.reservation,
            Some(ClaimReservationState::Settled {
                outcome: ClaimReservationOutcome::Refused
            })
        );
        assert_eq!(after.projection, before.projection);
        assert_finalized(&before, &after)?;
        fixture
            .runtime
            .on_call(cancel)
            .await
            .map_err(|error| format!("settlement replay: {error:?}"))?;
        assert_eq!(inspect(&fixture).await?, after);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn ordinary_unheld_projection_and_archive_completion_roll_back_and_retry_together()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        fixture.import(&bundle()?).await?;
        fixture
            .fleet_request(&request(FleetOperation::WorkspaceArchive {})?)
            .await?;
        let before = inspect(&fixture).await?;
        assert!(before.projection.is_none());
        let projection = unheld_call(&fixture).await?;
        board_write_fault(&fixture, true).await?;
        assert!(matches!(
            fixture.runtime.on_call(projection.clone()).await,
            Err(CallFailure::AnswerLost)
        ));
        assert_eq!(inspect(&fixture).await?, before);
        board_write_fault(&fixture, false).await?;
        let bytes = fixture
            .runtime
            .on_call(projection.clone())
            .await
            .map_err(|error| format!("projection retry: {error:?}"))?;
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes)?,
            json!({"kind":"applied"})
        );
        let after = inspect(&fixture).await?;
        assert!(after.projection.is_some());
        assert!(after.reservation.is_none());
        assert_finalized(&before, &after)?;
        let bytes = fixture
            .runtime
            .on_call(projection)
            .await
            .map_err(|error| format!("projection replay: {error:?}"))?;
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes)?,
            json!({"kind":"duplicate"})
        );
        assert_eq!(inspect(&fixture).await?, after);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn final_report_and_archive_completion_roll_back_and_retry_together() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        fixture.import(&bundle()?).await?;
        fixture
            .fleet_request(&request(FleetOperation::WorkspaceArchive {})?)
            .await?;
        let before = inspect(&fixture).await?;
        let call = archive_report_call(PROJECT)?;
        board_write_fault(&fixture, true).await?;
        assert!(matches!(
            fixture.runtime.on_call(call.clone()).await,
            Err(CallFailure::AnswerLost)
        ));
        assert_eq!(inspect(&fixture).await?, before);
        board_write_fault(&fixture, false).await?;
        let bytes = fixture
            .runtime
            .on_call(call.clone())
            .await
            .map_err(|error| format!("report retry: {error:?}"))?;
        assert_eq!(
            serde_json::from_slice::<fleet_admission::ArchiveReportOutcome>(&bytes)?,
            fleet_admission::ArchiveReportOutcome::Recorded
        );
        let after = inspect(&fixture).await?;
        assert_finalized(&before, &after)?;
        assert_eq!(after.reports.len(), 1);
        let stored: Value =
            serde_json::from_str(after.reports.first().ok_or("native report absent")?)?;
        assert_eq!(
            stored.get("position"),
            Some(&json!({"epoch":7,"sequence":9_007_199_254_740_993_u64}))
        );
        assert_eq!(
            stored.get("installation"),
            Some(&json!("550e8400-e29b-41d4-a716-4466554400a9"))
        );
        let bytes = fixture
            .runtime
            .on_call(call)
            .await
            .map_err(|error| format!("report replay: {error:?}"))?;
        assert_eq!(
            serde_json::from_slice::<fleet_admission::ArchiveReportOutcome>(&bytes)?,
            fleet_admission::ArchiveReportOutcome::AlreadyRecorded
        );
        assert_eq!(inspect(&fixture).await?, after);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof
}

fn archive_report_call(project: &str) -> TestResult<Vec<u8>> {
    Ok(serde_json::to_vec(&json!({
        "source":{"kind":"connection","value":"550e8400-e29b-41d4-a716-4466554400a9"},"sourceFace":"session","authority":{"kind":"object"},
        "destination":{"kind":"fleet"},"destinationFace":"fleet",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"archive_report"},
        "kind":"central.archive-report","target":null,"request":{
            "board":"019f0000-0000-7000-8000-000000000a82","project":project,
            "position":{"epoch":7,"sequence":9_007_199_254_740_993_u64}
        }
    }))?)
}

#[tokio::test]
async fn stored_legacy_and_mixed_positions_cannot_count_as_archive_reports() -> TestResult {
    for invalid_position in [
        json!({"epoch":7,"commitId":11}),
        json!({"epoch":7,"sequence":11,"commitId":11}),
    ] {
        let root = tempfile::tempdir()?;
        let fixture = Fixture::open(root.path()).await?;
        let proof = async {
            const SECOND: &str = "019f0000-0000-7000-8000-000000000a84";
            let mut offered = bundle()?;
            offered.records.push(serde_json::from_value(json!({
                "kind":"directory","project":SECOND,"board":"019f0000-0000-7000-8000-000000000a82",
                "title":"Other archive blocker","time":{"kind":"legacyUnknown"}
            }))?);
            fixture.import(&offered).await?;
            fixture.fleet_request(&request(FleetOperation::WorkspaceArchive {})?).await?;
            let bytes = fixture.runtime.on_call(archive_report_call(PROJECT)?).await.map_err(|error| format!("partial report: {error:?}"))?;
            assert_eq!(serde_json::from_slice::<fleet_admission::ArchiveReportOutcome>(&bytes)?, fleet_admission::ArchiveReportOutcome::Recorded);
            let state = inspect(&fixture).await?;
            let mut stored: Value = serde_json::from_str(state.reports.first().ok_or("partial report absent")?)?;
            stored["position"] = invalid_position;
            let payload = serde_json::to_string(&stored)?;
            fixture.object.transaction(move |scope|Box::pin(async move {
                query::<sqlx_turso::Turso>("UPDATE archive_reports SET report=? WHERE project_id=?").bind(payload).bind(PROJECT).execute(scope.executor()).await.map_err(StoreError::backend)?;
                Ok(())
            })).await?;
            let before = inspect(&fixture).await?;
            assert!(matches!(fixture.runtime.on_call(archive_report_call(SECOND)?).await, Err(CallFailure::AnswerLost)));
            assert_eq!(inspect(&fixture).await?, before);
            assert!(before.records.iter().all(|record|matches!(record,Record::Board(board) if board.state == BoardState::Archiving) || matches!(record,Record::Workspace(workspace) if workspace.state == WorkspaceState::Archiving)));
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }.await;
        fixture.objects.close().await?;
        proof?;
    }
    Ok(())
}
