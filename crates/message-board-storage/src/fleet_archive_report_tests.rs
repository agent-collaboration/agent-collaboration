use crate::FleetStore;
use collaboration_replication_protocol::{Epoch, ReplicationPosition};
use fleet_admission::{
    ArchiveReport, ArchiveReportOutcome, FleetIds,
    conformance_cases::{FleetCaseAction, cases},
};
use message_board::{Record, RecordId, ServiceId};
use sqlx::Connection;
use sqlx_turso::{TursoConnectOptions, TursoConnection};
use std::path::Path;

type TestResult<TValue = ()> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;

async fn fixture() -> TestResult<(tempfile::TempDir, FleetStore, Vec<ArchiveReport>)> {
    let directory = tempfile::tempdir()?;
    let store = FleetStore::open(&directory.path().join("fleet.sqlite")).await?;
    let case = cases()?
        .into_iter()
        .find(|case| case.name == "metadata-and-archive-reports")
        .ok_or("archive case")?;
    let mut reports = Vec::new();
    let mut archive_request = None;
    for step in case.steps {
        match step.action {
            FleetCaseAction::Request { request, .. } if reports.is_empty() => {
                if matches!(
                    request.operation,
                    fleet_admission::FleetOperation::WorkspaceCreate { .. }
                ) {
                    archive_request = Some(request.clone());
                }
                store
                    .admit(
                        &request,
                        "2026-10-12T00:00:00Z".parse()?,
                        &FleetIds {
                            generated_record: RecordId::generate(),
                        },
                    )
                    .await?;
            }
            FleetCaseAction::ArchiveReport { report, .. } => reports.push(report),
            FleetCaseAction::Request { .. } => {}
        }
    }
    let mut archive_request = archive_request.ok_or("workspace request")?;
    archive_request.request_id = message_board::RequestId::generate();
    archive_request.operation = fleet_admission::FleetOperation::WorkspaceArchive {};
    store
        .admit(
            &archive_request,
            "2026-10-12T00:00:00Z".parse()?,
            &FleetIds {
                generated_record: RecordId::generate(),
            },
        )
        .await?;
    // The shared table's middle report is the duplicate of its first project.
    let first = reports.first().ok_or("first report")?.clone();
    let last = reports.last().ok_or("last report")?.clone();
    Ok((directory, store, vec![first, last]))
}

async fn stored(path: &Path, report: &ArchiveReport) -> TestResult<Vec<ArchiveReport>> {
    let mut connection =
        TursoConnection::connect_with(&TursoConnectOptions::new().filename(path)).await?;
    let reports = crate::fleet_archive_reports::reports(&mut connection, &report.board).await?;
    connection.close().await?;
    Ok(reports)
}

#[tokio::test]
async fn first_report_keeps_position_installation_and_source_time_after_reopen_and_successor_repeat()
-> TestResult {
    let (directory, store, reports) = fixture().await?;
    let path = directory.path().join("fleet.sqlite");
    let first = reports.first().ok_or("first")?.clone();
    let snapshot = store.snapshot_records().await?;
    assert_eq!(
        store.report_archive_seen(&first).await?,
        ArchiveReportOutcome::Recorded
    );
    assert_eq!(
        store.snapshot_records().await?,
        snapshot,
        "partial report writes no record/feed/ledger"
    );
    assert_eq!(stored(&path, &first).await?, vec![first.clone()]);
    store.close().await;
    let store = FleetStore::open(&path).await?;
    let mut successor = first.clone();
    successor.installation =
        ServiceId::try_from("00000000-0000-4000-8000-000000000002".to_owned())?;
    successor.position = ReplicationPosition::new(Epoch::new(9)?, 99.into());
    successor.reported_at += chrono::TimeDelta::days(1);
    assert_eq!(
        store.report_archive_seen(&successor).await?,
        ArchiveReportOutcome::AlreadyRecorded
    );
    assert_eq!(stored(&path, &first).await?, vec![first]);
    assert_eq!(store.snapshot_records().await?, snapshot);
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn a_finalizing_feed_constraint_rolls_report_and_both_archive_records_back() -> TestResult {
    let (directory, store, reports) = fixture().await?;
    let path = directory.path().join("fleet.sqlite");
    let first = reports.first().ok_or("first")?;
    let last = reports.last().ok_or("last")?;
    assert_eq!(
        store.report_archive_seen(first).await?,
        ArchiveReportOutcome::Recorded
    );
    let before = store.snapshot_records().await?;
    let mut connection =
        TursoConnection::connect_with(&TursoConnectOptions::new().filename(&path)).await?;
    // Occupy the exact next feed key to cause a real late storage constraint,
    // after the report and both finalizing record writes have been attempted.
    let blocked = i64::try_from(before.through.successor()?.get())?;
    sqlx::query("INSERT INTO fleet_feed(position, entry) VALUES (?, ?)")
        .bind(blocked)
        .bind("injected occupied feed key")
        .execute(&mut connection)
        .await?;
    assert!(store.report_archive_seen(last).await.is_err());
    assert_eq!(store.snapshot_records().await?, before);
    assert_eq!(stored(&path, first).await?, vec![first.clone()]);
    sqlx::query("DELETE FROM fleet_feed WHERE position=?")
        .bind(blocked)
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    assert_eq!(
        store.report_archive_seen(last).await?,
        ArchiveReportOutcome::Recorded
    );
    let after = store.snapshot_records().await?;
    assert_eq!(after.through, before.through.successor()?.successor()?);
    let ledger = |records: &[Record]| {
        records
            .iter()
            .filter(|record| matches!(record, Record::Request(_)))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(ledger(&after.records), ledger(&before.records));
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn malformed_automated_report_rows_never_count_as_successful_reports() -> TestResult {
    for mutation in [
        "UPDATE archive_reports SET position_epoch='0'",
        "UPDATE archive_reports SET position_commit='invalid'",
        "UPDATE archive_reports SET installation='invalid'",
        "UPDATE archive_reports SET reported_at='invalid'",
    ] {
        let (directory, store, reports) = fixture().await?;
        let first = reports.first().ok_or("first")?;
        let last = reports.last().ok_or("last")?;
        store.report_archive_seen(first).await?;
        let before = store.snapshot_records().await?;
        let mut connection = TursoConnection::connect_with(
            &TursoConnectOptions::new().filename(directory.path().join("fleet.sqlite")),
        )
        .await?;
        sqlx::query(mutation).execute(&mut connection).await?;
        assert!(
            matches!(
                store.report_archive_seen(last).await,
                Err(crate::FleetStorageError::InvalidRecord)
            ),
            "{mutation}"
        );
        assert_eq!(store.snapshot_records().await?, before);
        connection.close().await?;
        store.close().await;
    }
    Ok(())
}
