//! A late archive write failure rolls back the mutation that removed its blocker.
use super::{
    metadata::request,
    support::{Fixture, TestResult, bundle},
};
use collaboration_central_core::ClaimReservationState;
use durable_steps::{AtomicStore, CallFailure, StoreError, storage::TransactionScope};
use fleet_admission::FleetOperation;
use message_board::{BoardState, FleetPosition, Record, RecordPlacement, WorkspaceState};
use serde_json::{Value, json};
use sqlx_core::{executor::Executor, query::query, row::Row};

const PROJECT: &str = "019f0000-0000-7000-8000-000000000a83";
const ATTEMPT: &str = "019f0000-0000-7000-8000-000000000b01";

#[derive(Debug, PartialEq)]
struct ArchiveState {
    records: Vec<Record>,
    feed: Vec<(i64, String)>,
    ledgers: Vec<String>,
    reports: Vec<Value>,
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
        let reports = query::<sqlx_turso::Turso>("SELECT report_id,board_id,project_id,epoch,commit_sequence,installation,reported_at,created_at,updated_at FROM fleet_archive_reports ORDER BY board_id,project_id").fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter().map(|row|Ok::<_,StoreError>(json!({
            "report_id":row.try_get::<String,_>("report_id").map_err(StoreError::backend)?,
            "board_id":row.try_get::<String,_>("board_id").map_err(StoreError::backend)?,
            "project_id":row.try_get::<String,_>("project_id").map_err(StoreError::backend)?,
            "epoch":row.try_get::<String,_>("epoch").map_err(StoreError::backend)?,
            "commit_sequence":row.try_get::<String,_>("commit_sequence").map_err(StoreError::backend)?,
            "installation":row.try_get::<String,_>("installation").map_err(StoreError::backend)?,
            "reported_at":row.try_get::<String,_>("reported_at").map_err(StoreError::backend)?,
            "created_at":row.try_get::<i64,_>("created_at").map_err(StoreError::backend)?,
            "updated_at":row.try_get::<i64,_>("updated_at").map_err(StoreError::backend)?,
        }))).collect::<Result<Vec<_>,_>>()?;
        let row = query::<sqlx_turso::Turso>("SELECT d.lease_projection,r.state_kind,r.outcome FROM project_directory d LEFT JOIN lease_claim_reservations r ON r.project_id=d.project_id AND r.attempt_id=? WHERE d.project_id=?").bind(ATTEMPT).bind(PROJECT).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        let reservation = crate::typed_claim_observer::reservation_state(&row)?.map(|value|serde_json::from_value(value).map_err(StoreError::backend)).transpose()?;
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
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
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

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
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
        let stored = after.reports.first().ok_or("native report absent")?;
        assert_eq!(stored.get("epoch"), Some(&json!("7")));
        assert_eq!(
            stored.get("commit_sequence"),
            Some(&json!("9007199254740993"))
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
        "source":{"kind":"fleet"},"sourceFace":"session","authority":{"kind":"machine","value":{"installation":"550e8400-e29b-41d4-a716-4466554400a9"}},
        "destination":{"kind":"fleet"},"destinationFace":"fleet",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"archive_report"},
        "kind":"central.archive-report","target":null,"request":{
            "board":"019f0000-0000-7000-8000-000000000a82","project":project,
            "position":{"epoch":7,"sequence":9_007_199_254_740_993_u64}
        }
    }))?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn invalid_stored_native_positions_cannot_count_as_archive_reports() -> TestResult {
    for (epoch, sequence) in [
        ("0", "11"),
        ("7", "legacy commit id"),
        ("7", "01"),
        ("18446744073709551616", "11"),
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
            fixture.object.transaction(move |scope|Box::pin(async move {
                query::<sqlx_turso::Turso>("UPDATE fleet_archive_reports SET epoch=?,commit_sequence=? WHERE project_id=?").bind(epoch).bind(sequence).bind(PROJECT).execute(scope.executor()).await.map_err(StoreError::backend)?;
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
