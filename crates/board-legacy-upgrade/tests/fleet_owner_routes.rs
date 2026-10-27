#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! The retained Fleet obligations run on current owner and normal-session routes.
#![allow(clippy::panic_in_result_fn)]
#[path = "upload_support/central_fixture.rs"]
mod central_fixture;
mod support;
#[path = "upload_support/mod.rs"]
#[allow(
    dead_code,
    reason = "Other shared helpers serve the project publication tests."
)]
mod upload_support;

use collaboration_central_conformance::{
    ConformanceClient, JwksFixtureServer, run_composed_fleet_conflict,
};
use fleet_admission::{
    FleetImportCounts, FleetImportRecord, FleetImportRefusal, FleetImportResult,
};
use message_board::{Description, HumanId, MachineLabel, WorkspaceId};
use support::*;

const COUNTS: &str = "SELECT json_object('workspaces',(SELECT count(*) FROM workspaces),'boards',(SELECT count(*) FROM boards),'directoryEntries',(SELECT count(*) FROM project_directory),'feedRows',(SELECT count(*) FROM fleet_feed)) AS value";
const APPLICATION_STATE: &str = "SELECT 'workspace:' || json_object('id',workspace_id,'name',name,'payload',payload,'time',router_time,'basis',time_basis,'installation',origin_installation,'machine',origin_machine,'digest',import_digest) AS value FROM workspaces UNION ALL SELECT 'board:' || json_object('id',board_id,'name',name,'payload',payload,'time',router_time,'basis',time_basis,'installation',origin_installation,'machine',origin_machine,'digest',import_digest) FROM boards UNION ALL SELECT 'directory:' || json_object('id',project_id,'name',name,'payload',payload,'time',router_time,'basis',time_basis,'installation',origin_installation,'machine',origin_machine,'digest',import_digest) FROM project_directory UNION ALL SELECT 'feed:' || json_object('id',entry_id,'position',position,'record',record_id,'payload',payload,'time',router_time,'basis',time_basis,'installation',origin_installation,'machine',origin_machine,'asOf',as_of) FROM fleet_feed UNION ALL SELECT 'head:' || json_object('id',state_id,'name',name,'position',last_position,'floor',retained_through) FROM fleet_feed_state ORDER BY value";

#[derive(Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FleetApplicationCounts {
    workspaces: u64,
    boards: u64,
    directory_entries: u64,
    feed_rows: u64,
}

async fn counts(root: &std::path::Path) -> TestResult<FleetApplicationCounts> {
    let rows = upload_support::read(root, "fleet", COUNTS).await?;
    Ok(serde_json::from_str(
        rows.first().ok_or("Fleet count row missing")?,
    )?)
}

#[tokio::test]
async fn actual_imported_fleet_bundle_is_atomic_and_retains_first_records_on_replay_and_conflict()
-> TestResult {
    // Arrange: D's actual bundle, not a second synthetic Fleet transformation.
    // Fixture source counts independently establish the expected 2/2/3 records.
    let prepared = prepare("legacy-complete.sqlite").await?;
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let server = central_fixture::start_server(root.path(), &jwks).await?;
    let observed: TestResult = async {
        let client = ConformanceClient::new(server.base_url())
            .map_err(|error| error as Box<dyn std::error::Error>)?;
        let mut broken = prepared.report.fleet.clone();
        let absent = WorkspaceId::generate();
        let board = broken
            .records
            .iter_mut()
            .find_map(|record| match record {
                FleetImportRecord::Board { workspace, .. } => Some(workspace),
                _ => None,
            })
            .ok_or("fixture Board missing")?;
        *board = absent.clone();

        // Act / Assert: one missing parent must roll back every offered record
        // and every application feed entry, even valid earlier workspace rows.
        let refused = client
            .import_fleet(&broken)
            .await
            .map_err(|error| error as Box<dyn std::error::Error>)?;
        assert!(matches!(refused,FleetImportResult::Refused {conflicts}
        if conflicts.iter().any(|conflict|matches!(&conflict.reason,
            FleetImportRefusal::ParentMissing {parent} if *parent==absent.clone().into()
        ))));
        assert_eq!(
            counts(root.path()).await?,
            FleetApplicationCounts {
                workspaces: 0,
                boards: 0,
                directory_entries: 0,
                feed_rows: 0,
            }
        );
        // Fail during real writes too: pure admission rejection alone cannot
        // prove rollback of earlier valid workspace inserts and feed entries.
        upload_support::execute(root.path(), "fleet",
            "CREATE TRIGGER interrupt_fleet_import BEFORE INSERT ON boards BEGIN SELECT RAISE(ABORT,'Fleet import write interrupted'); END").await?;
        let interrupted = central_fixture::owner_client()?
            .post(format!("{}/admin/v1/import/fleet", server.base_url()))
            .json(&prepared.report.fleet)
            .send().await;
        let released = upload_support::execute(root.path(), "fleet", "DROP TRIGGER interrupt_fleet_import").await;
        released?;
        assert_eq!(interrupted?.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(counts(root.path()).await?, FleetApplicationCounts {
            workspaces: 0, boards: 0, directory_entries: 0, feed_rows: 0,
        });
        assert_eq!(
            client
                .import_fleet(&prepared.report.fleet)
                .await
                .map_err(|error| error as Box<dyn std::error::Error>)?,
            FleetImportResult::Imported {
                counts: FleetImportCounts {
                    workspaces: 2,
                    boards: 2,
                    directory_entries: 3
                }
            }
        );
        assert_eq!(
            counts(root.path()).await?,
            FleetApplicationCounts {
                workspaces: 2,
                boards: 2,
                directory_entries: 3,
                feed_rows: 7,
            }
        );
        let before = upload_support::read(root.path(), "fleet", APPLICATION_STATE).await?;
        let mut relabelled = prepared.report.fleet.clone();
        relabelled.declared_owner = HumanId::try_from("different declared importer".to_owned())?;
        relabelled.provenance.machine =
            MachineLabel::try_from("different source label".to_owned())?;
        assert_eq!(
            client
                .import_fleet(&relabelled)
                .await
                .map_err(|error| error as Box<dyn std::error::Error>)?,
            FleetImportResult::AlreadyImported
        );
        assert_eq!(
            upload_support::read(root.path(), "fleet", APPLICATION_STATE).await?,
            before
        );
        let mut changed = prepared.report.fleet.clone();
        let description = changed
            .records
            .iter_mut()
            .find_map(|record| match record {
                FleetImportRecord::Workspace { description, .. } => Some(description),
                _ => None,
            })
            .ok_or("fixture Workspace missing")?;
        *description =
            Description::try_from("Changed actual legacy workspace description".to_owned())?;
        assert!(matches!(
            client
                .import_fleet(&changed)
                .await
                .map_err(|error| error as Box<dyn std::error::Error>)?,
            FleetImportResult::Refused { .. }
        ));
        assert_eq!(
            upload_support::read(root.path(), "fleet", APPLICATION_STATE).await?,
            before
        );
        Ok(())
    }
    .await;
    prepared.close().await;
    let stopped = server.shutdown().await;
    let keys_stopped = jwks.shutdown().await;
    stopped?;
    keys_stopped.map_err(|error| error as Box<dyn std::error::Error>)?;
    observed
}

#[tokio::test]
async fn normal_fleet_conflict_preserves_exact_decision_records_and_feed() -> TestResult {
    // Reuse the current real-session corpus replacing the deleted LocalServer
    // wrapper; no second decision model or fake Fleet snapshot is introduced.
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let server = central_fixture::start_server(root.path(), &jwks).await?;
    let client = ConformanceClient::new(server.base_url())
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let observed = run_composed_fleet_conflict(&client).await;
    let stopped = server.shutdown().await;
    let keys_stopped = jwks.shutdown().await;
    stopped?;
    keys_stopped.map_err(|error| error as Box<dyn std::error::Error>)?;
    observed.map_err(|error| error as Box<dyn std::error::Error>)
}
