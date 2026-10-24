//! Measure the actual single-request Fleet boundary without weakening its limit.
#![allow(clippy::panic_in_result_fn)]
#[path = "upload_support/central_fixture.rs"]
mod central_fixture;

use collaboration_central_conformance::JwksFixtureServer;
use fleet_admission::{FleetImportBundle, FleetImportCounts, FleetImportRecord, FleetImportResult};
use message_board::{
    Description, HumanId, MachineLabel, RecordTime, ResourceName, ServiceId, WorkspaceId,
};
use sqlx_turso::sqlx::{ConnectOptions, Connection, Row};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn oversized_valid_fleet_bundle_refuses_before_partial_writes_and_a_small_control_imports()
-> TestResult {
    // Arrange: valid public domain records exceed the route's existing 2 MiB
    // single-body limit. No invalid names, parent links or credentials cause it.
    let description = Description::try_from("x".repeat(16_384))?;
    let mut records = Vec::new();
    for index in 0..128 {
        records.push(FleetImportRecord::Workspace {
            id: WorkspaceId::generate(),
            name: ResourceName::try_from(format!("Synthetic workspace {index}"))?,
            description: description.clone(),
            repositories: Vec::new(),
            time: RecordTime::LegacyUnknown,
        });
    }
    let bundle = FleetImportBundle {
        provenance: fleet_admission::ImportProvenance {
            installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440000".to_owned())?,
            machine: MachineLabel::try_from("Fleet body measurement".to_owned())?,
        },
        declared_owner: HumanId::try_from("fixture-owner".to_owned())?,
        records,
    };
    let bytes = serde_json::to_vec(&bundle)?;
    assert!(bytes.len() > 2_097_152);
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let server = central_fixture::start_server(root.path(), &jwks).await?;
    let observed: TestResult = async {
        let http = central_fixture::owner_client()?;
        let route = format!("{}/admin/v1/import/fleet", server.base_url());
        let fleet_path = root.path().join("fleet.db");
        assert!(
            !fleet_path.exists(),
            "fresh target unexpectedly has Fleet state"
        );

        // Act / Assert: exercise the production owner route and report only
        // request size/status, then independently check no Fleet file was made.
        let response = http
            .post(&route)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(bytes.clone())
            .send()
            .await?;
        eprintln!(
            "Fleet bundle records={} bytes={} status={}",
            bundle.records.len(),
            bytes.len(),
            response.status().as_u16()
        );
        assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
        assert!(
            !fleet_path.exists(),
            "oversized request created partial Fleet state"
        );

        // Positive control: the same actual route and owner credentials admit
        // a valid smaller bundle; a route/authentication failure cannot pass.
        let small = FleetImportBundle {
            records: bundle.records[..1].to_vec(),
            ..bundle.clone()
        };
        let answer: FleetImportResult = http
            .post(route)
            .json(&small)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(
            answer,
            FleetImportResult::Imported {
                counts: FleetImportCounts {
                    workspaces: 1,
                    boards: 0,
                    directory_entries: 0
                }
            }
        );
        assert!(fleet_path.exists());
        let mut readback = sqlx_turso::TursoConnectOptions::new()
            .filename(&fleet_path)
            .read_only(true)
            .connect()
            .await?;
        let counts = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
            "SELECT (SELECT count(*) FROM workspaces) AS workspace_count, (SELECT count(*) FROM fleet_feed) AS feed_count",
        )
        .fetch_one(&mut readback)
        .await?;
        let closed = readback.close().await;
        assert_eq!(counts.try_get::<i64, _>("workspace_count")?, 1);
        assert_eq!(counts.try_get::<i64, _>("feed_count")?, 1);
        closed?;
        Ok(())
    }
    .await;
    let stopped = server.shutdown().await;
    let keys_stopped = jwks.shutdown().await;
    stopped?;
    keys_stopped.map_err(|error| error as Box<dyn std::error::Error>)?;
    observed
}
