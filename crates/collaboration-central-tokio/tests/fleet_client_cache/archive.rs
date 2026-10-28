//! Real FleetClient, pump, native session and owning store retain archive outcomes.
use super::*;
use fleet_admission::{ArchiveReport, ArchiveReportOutcome, ArchiveReportRefusal};
use fleet_sync_protocol::{CommitSequence, Epoch, ReplicationPosition};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};

const BOARD: &str = "ffffffff-ffff-7fff-8fff-fffffffffca3";
const FIRST: &str = "ffffffff-ffff-7fff-8fff-fffffffffca4";
const SECOND: &str = "ffffffff-ffff-7fff-8fff-fffffffffca5";

fn report(project: &str) -> TestResult<ArchiveReport> {
    Ok(ArchiveReport {
        board: BoardId::try_from(BOARD.to_owned())?,
        project: ProjectId::try_from(project.to_owned())?,
        position: ReplicationPosition::new(
            Epoch::new(7)?,
            CommitSequence::new(9_007_199_254_740_993),
        ),
        installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440bad".to_owned())?,
        reported_at: "2000-01-01T00:00:00Z".parse()?,
    })
}
async fn stored(root: &Path) -> TestResult<(Vec<serde_json::Value>, Vec<String>)> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet.db"))
        .read_only(true)
        .connect()
        .await?;
    let rows = query::<sqlx_turso::Turso>("SELECT report FROM archive_reports ORDER BY project_id")
        .fetch_all(&mut connection)
        .await?;
    let reports = rows
        .into_iter()
        .map(|row| {
            Ok(serde_json::from_str::<serde_json::Value>(
                &row.try_get::<String, _>("report")?,
            )?)
        })
        .collect::<TestResult<Vec<_>>>()?;
    let rows = query::<sqlx_turso::Turso>(
        "SELECT payload FROM fleet_request_decisions ORDER BY request_id",
    )
    .fetch_all(&mut connection)
    .await?;
    let ledgers = rows
        .into_iter()
        .map(|row| row.try_get::<String, _>("payload"))
        .collect::<Result<Vec<_>, _>>()?;
    connection.close().await?;
    Ok((reports, ledgers))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn native_archive_reports_keep_positions_and_typed_outcomes_through_the_real_client()
-> TestResult {
    let root = tempfile::tempdir()?;
    let server_root = root.path().join("central");
    let router_root = root.path().join("router");
    let jwks = JwksFixtureServer::start().await?;
    let mut live = fixture::Live::start(&server_root, &jwks.url).await?;
    let reader = fixture::Reader::start(&router_root, live.server.base_url()).await?;
    let proof = async {
        live.commit(fixture::create()?).await?;
        live.commit(fixture::request(FleetOperation::BoardCreate {
            board: BoardId::try_from(BOARD.to_owned())?,
            name: ResourceName::try_from("Archive board".to_owned())?,
            description: Description::try_from("native archive proof".to_owned())?,
            board_kind: BoardKind::Area {},
        })?).await?;
        for (project,title) in [(FIRST,"First project"),(SECOND,"Second project")] {
            live.commit(fixture::request(FleetOperation::ProjectDirectoryCreate {
                project: ProjectId::try_from(project.to_owned())?,
                board: BoardId::try_from(BOARD.to_owned())?,
                title: ResourceName::try_from(title.to_owned())?,
            })?).await?;
        }
        assert_eq!(reader.fleet.report_archive_seen(report(FIRST)?).await?,ArchiveReportOutcome::Refused(ArchiveReportRefusal::BoardNotArchiving { state: BoardState::Open }));
        let archive = fixture::request(FleetOperation::WorkspaceArchive {})?;
        let actor = archive.actor.clone();
        live.commit(archive).await?;
        let before = stored(&server_root).await?;
        assert!(before.0.is_empty());
        assert_eq!(reader.fleet.report_archive_seen(report(FIRST)?).await?,ArchiveReportOutcome::Recorded);
        let first = stored(&server_root).await?;
        assert_eq!(first.0.len(),1);
        let native = first.0.first().ok_or("stored native report absent")?;
        assert_eq!(native.get("position"),Some(&serde_json::json!({"epoch":7,"sequence":9_007_199_254_740_993_u64})));
        assert_eq!(native.get("installation"),Some(&serde_json::json!(READER_SERVICE)));
        assert_ne!(native.get("reportedAt"),Some(&serde_json::json!("2000-01-01T00:00:00Z")));
        assert_eq!(first.1,before.1);
        let mut repeated = report(FIRST)?;
        repeated.position = ReplicationPosition::new(Epoch::new(99)?,CommitSequence::new(4));
        assert_eq!(reader.fleet.report_archive_seen(repeated).await?,ArchiveReportOutcome::AlreadyRecorded);
        assert_eq!(stored(&server_root).await?,first);
        reader.fleet.synchronize().await?;
        assert!(matches!(reader.fleet.record(&BoardId::try_from(BOARD.to_owned())?.into()).await?,Some(Record::Board(board)) if board.state == BoardState::Archiving));
        assert_eq!(reader.fleet.report_archive_seen(report(SECOND)?).await?,ArchiveReportOutcome::Recorded);
        let complete = stored(&server_root).await?;
        assert_eq!(complete.0.len(),2);
        assert_eq!(complete.1,before.1);
        reader.fleet.synchronize().await?;
        for reference in [BoardId::try_from(BOARD.to_owned())?.into(), WorkspaceId::try_from(WORKSPACE.to_owned())?.into()] {
            let record = reader.fleet.record(&reference).await?.ok_or("finalized record absent")?;
            assert!(matches!(&record,Record::Board(board) if board.state == BoardState::Archived) || matches!(&record,Record::Workspace(workspace) if workspace.state == WorkspaceState::Archived));
            assert_eq!(record.envelope().actor,actor);
            assert!(record.envelope().request.is_none());
            assert_eq!(record.envelope().position.origin().ok_or("archive origin absent")?.installation.as_str(),WRITER_SERVICE);
        }
        reader.ingress.controls.corrupt_next_archive_outcome();
        assert!(matches!(reader.fleet.report_archive_seen(report(FIRST)?).await,Err(collaboration_runtime::FleetClientError::Codec(_))));
        assert_eq!(stored(&server_root).await?,complete);
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let reader_closed = reader.shutdown().await;
    let live_closed = live.shutdown().await;
    let jwks_closed = jwks.shutdown().await;
    proof?;
    reader_closed?;
    live_closed?;
    jwks_closed?;
    Ok(())
}
