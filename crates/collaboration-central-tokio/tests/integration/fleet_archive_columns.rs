//! Typed archive reports preserve native positions, first acknowledgements and atomic completion.
use super::composed_fleet_fixture::*;
use durable_steps::{
    AtomicStore, StoreError,
    storage::{JournalStorage, TransactionScope},
};
use fleet_admission::{ArchiveReportOutcome, FleetOperation};
use serde_json::json;
use sqlx_core::{query::query, row::Row};

#[derive(Debug, PartialEq)]
struct ReportColumns {
    id: String,
    board: String,
    project: String,
    epoch: String,
    sequence: String,
    installation: String,
    reported_at: String,
    created_at: i64,
    updated_at: i64,
}
async fn columns(fixture: &Fixture) -> TestResult<ReportColumns> {
    Ok(fixture.store.transaction(|scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT report_id,board_id,project_id,epoch,commit_sequence,installation,reported_at,created_at,updated_at FROM fleet_archive_reports").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(ReportColumns {id:row.try_get("report_id").map_err(StoreError::backend)?,board:row.try_get("board_id").map_err(StoreError::backend)?,project:row.try_get("project_id").map_err(StoreError::backend)?,epoch:row.try_get("epoch").map_err(StoreError::backend)?,sequence:row.try_get("commit_sequence").map_err(StoreError::backend)?,installation:row.try_get("installation").map_err(StoreError::backend)?,reported_at:row.try_get("reported_at").map_err(StoreError::backend)?,created_at:row.try_get("created_at").map_err(StoreError::backend)?,updated_at:row.try_get("updated_at").map_err(StoreError::backend)?})
    })).await?)
}
async fn prepare(fixture: &Fixture) -> TestResult {
    fixture.arrange_registered().await?;
    let answer = fixture
        .metadata(FleetOperation::WorkspaceArchive {})
        .await?;
    if !matches!(
        serde_json::from_value::<fleet_admission::FleetRequestResult>(answer)?,
        fleet_admission::FleetRequestResult::Decided(
            message_board::RequestDecision::Accepted { .. }
        )
    ) {
        return Err("archive request not accepted".into());
    }
    Ok(())
}
fn report_call() -> TestResult<Vec<u8>> {
    Ok(serde_json::to_vec(&json!({
        "source":{"kind":"fleet"},"sourceFace":"session","authority":{"kind":"machine","value":{"installation":SERVICE_A}},
        "destination":{"kind":"fleet"},"destinationFace":"fleet",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"typed_archive_report"},
        "kind":"central.archive-report","target":null,
        "request":{"board":"019f0000-0000-7000-8000-000000008182","project":PROJECT,"position":{"epoch":u64::MAX,"sequence":u64::MAX}}
    }))?)
}
async fn report(fixture: &Fixture, call: &[u8]) -> TestResult<ArchiveReportOutcome> {
    let bytes = fixture
        .runtime
        .on_call(call.to_vec())
        .await
        .map_err(|error| format!("archive report: {error:?}"))?;
    Ok(serde_json::from_slice(&bytes)?)
}
#[derive(Debug, PartialEq)]
struct ArchiveSnapshot {
    reports: i64,
    feed: i64,
    notices: i64,
    records: Vec<String>,
    wake_at: Option<runtime_primitives::SyncTime>,
}
async fn snapshot(fixture: &Fixture) -> TestResult<ArchiveSnapshot> {
    Ok(fixture.store.transaction(|scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM fleet_archive_reports) AS reports,(SELECT last_position FROM fleet_feed_state WHERE name='fleet') AS feed,(SELECT count(*) FROM machine_control_notices) AS notices").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        let mut records=crate::fleet_record_rows::metadata_records(scope).await?.into_iter().map(|record|serde_json::to_string(&record).map_err(StoreError::backend)).collect::<Result<Vec<_>,_>>()?;
        records.sort();
        Ok(ArchiveSnapshot {reports:row.try_get("reports").map_err(StoreError::backend)?,feed:row.try_get("feed").map_err(StoreError::backend)?,notices:row.try_get("notices").map_err(StoreError::backend)?,records,wake_at:scope.earliest().await?})
    })).await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The full u64 native position and first acknowledgement must remain byte-stable across replay and owner reopen."
)]
#[tokio::test]
async fn native_archive_columns_keep_full_u64_positions_and_first_injected_times_after_reopen()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    prepare(&fixture).await?;
    fixture.io.advance(1_234);
    let call = report_call()?;
    assert_eq!(
        report(&fixture, &call).await?,
        ArchiveReportOutcome::Recorded
    );
    let original = columns(&fixture).await?;
    assert!(runtime_primitives::uuid_v7(durable_steps::Uuid::parse_str(
        &original.id
    )?));
    assert_eq!(
        (
            original.board.as_str(),
            original.project.as_str(),
            original.installation.as_str()
        ),
        ("019f0000-0000-7000-8000-000000008182", PROJECT, SERVICE_A)
    );
    assert_eq!(
        (&original.epoch, &original.sequence),
        (&u64::MAX.to_string(), &u64::MAX.to_string())
    );
    assert_eq!(
        (original.created_at, original.updated_at),
        (NOW + 1_234, NOW + 1_234)
    );
    assert_eq!(
        original.reported_at,
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(NOW + 1_234)
            .ok_or("test timestamp out of range")?
            .to_rfc3339()
    );
    fixture.io.advance(2_000);
    assert_eq!(
        report(&fixture, &call).await?,
        ArchiveReportOutcome::AlreadyRecorded
    );
    assert_eq!(columns(&fixture).await?, original);
    fixture.close().await?;
    let reopened = Fixture::open(root.path(), NOW + 3_234).await?;
    assert_eq!(
        report(&reopened, &call).await?,
        ArchiveReportOutcome::AlreadyRecorded
    );
    assert_eq!(columns(&reopened).await?, original);
    reopened.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Invalid typed report columns must refuse the real archive read without changing records, feed or notices."
)]
#[tokio::test]
async fn invalid_archive_columns_cannot_count_as_a_completed_native_report() -> TestResult {
    for sql in [
        "UPDATE fleet_archive_reports SET epoch='0'",
        "UPDATE fleet_archive_reports SET commit_sequence='01'",
        "UPDATE fleet_archive_reports SET commit_sequence='18446744073709551616'",
        "UPDATE fleet_archive_reports SET updated_at=1.5",
        "UPDATE fleet_archive_reports SET created_at='bad timestamp'",
        "UPDATE fleet_archive_reports SET reported_at='bad timestamp'",
        "UPDATE fleet_archive_reports SET installation=' invalid installation '",
        "UPDATE fleet_archive_reports SET report_id='019f0000-0000-7000-0000-000000008301'",
    ] {
        let root = tempfile::tempdir()?;
        let fixture = Fixture::open(root.path(), NOW).await?;
        prepare(&fixture).await?;
        let call = report_call()?;
        assert_eq!(
            report(&fixture, &call).await?,
            ArchiveReportOutcome::Recorded
        );
        fixture.sql(sql).await?;
        let before = snapshot(&fixture).await?;
        assert!(
            report(&fixture, &call).await.is_err(),
            "accepted corrupt column: {sql}"
        );
        assert_eq!(snapshot(&fixture).await?, before);
        fixture.close().await?;
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A post-report board write failure must roll back the typed report and all finalization effects, then the same call commits once."
)]
#[tokio::test]
async fn typed_archive_report_and_finalization_roll_back_together_then_the_same_call_commits_once()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    prepare(&fixture).await?;
    let before = snapshot(&fixture).await?;
    let mut wakes = fixture.io.wakes()?;
    fixture.sql("CREATE TRIGGER reject_archive_completion BEFORE UPDATE ON boards BEGIN SELECT RAISE(ABORT,'injected post-report finalization failure'); END").await?;
    let call = report_call()?;
    assert!(report(&fixture, &call).await.is_err());
    assert_eq!(snapshot(&fixture).await?, before);
    // The outer runtime re-arms existing committed debt after every call,
    // including a refused mutation; that hint is distinct from new wake intent.
    if let Some(at) = before.wake_at {
        wakes.push(at);
    }
    assert_eq!(fixture.io.wakes()?, wakes);
    fixture
        .sql("DROP TRIGGER reject_archive_completion")
        .await?;
    assert_eq!(
        report(&fixture, &call).await?,
        ArchiveReportOutcome::Recorded
    );
    let after = snapshot(&fixture).await?;
    assert_eq!(after.reports, 1);
    assert!(after.feed > before.feed);
    assert_ne!(after.records, before.records);
    assert_eq!(
        report(&fixture, &call).await?,
        ArchiveReportOutcome::AlreadyRecorded
    );
    assert_eq!(snapshot(&fixture).await?, after);
    fixture.close().await
}
