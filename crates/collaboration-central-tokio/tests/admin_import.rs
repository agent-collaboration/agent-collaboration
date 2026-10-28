#![cfg(feature = "conformance")]

use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, fixture_policy,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_admission::{FleetImportBundle, FleetImportCounts, FleetImportResult};
use sqlx_core::{query::query, row::Row};
use sqlx_turso::sqlx::{ConnectOptions, Connection};
use std::{path::Path, time::Duration};

#[path = "native_http/support.rs"]
mod configuration;

fn fleet_bundle() -> ConformanceResult<FleetImportBundle> {
    Ok(serde_json::from_value(serde_json::json!({
        "provenance":{"installation":"550e8400-e29b-41d4-a716-446655440000","machine":"fixture source"},
        "declaredOwner":"importing-person",
        "records":[
            {"kind":"workspace","id":"019f0000-0000-7000-8000-000000000501","name":"Imported workspace","description":"Original description","repositories":[],"time":{"kind":"legacyUnknown"}},
            {"kind":"board","id":"019f0000-0000-7000-8000-000000000502","workspace":"019f0000-0000-7000-8000-000000000501","name":"Imported board","description":"","state":"active","time":{"kind":"legacyUnknown"}},
            {"kind":"directory","project":"019f0000-0000-7000-8000-000000000503","board":"019f0000-0000-7000-8000-000000000502","title":"Imported project","time":{"kind":"legacyUnknown"}}
        ]
    }))?)
}

#[derive(Debug, PartialEq)]
struct FleetReadback {
    records: Vec<message_board::Record>,
    rows: Vec<String>,
    feed: Vec<String>,
    feed_state: Vec<String>,
}

async fn readback(root: &Path) -> ConformanceResult<FleetReadback> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet.db"))
        .read_only(true)
        .connect()
        .await?;
    let record_rows = query::<sqlx_turso::Turso>(
        "SELECT json_object('id',workspace_id,'name',name,'payload',payload,'time',router_time,'basis',time_basis,'installation',origin_installation,'machine',origin_machine,'digest',import_digest) AS state,payload FROM workspaces UNION ALL SELECT json_object('id',board_id,'name',name,'payload',payload,'time',router_time,'basis',time_basis,'installation',origin_installation,'machine',origin_machine,'digest',import_digest),payload FROM boards UNION ALL SELECT json_object('id',project_id,'name',name,'payload',payload,'time',router_time,'basis',time_basis,'installation',origin_installation,'machine',origin_machine,'digest',import_digest),payload FROM project_directory ORDER BY state"
    ).fetch_all(&mut connection).await?;
    let mut records = Vec::new();
    let mut rows = Vec::new();
    for row in record_rows {
        records.push(serde_json::from_str(&row.try_get::<String, _>("payload")?)?);
        rows.push(row.try_get("state")?);
    }
    let feed = query::<sqlx_turso::Turso>("SELECT json_object('id',entry_id,'position',position,'record',record_id,'payload',payload,'time',router_time,'basis',time_basis,'installation',origin_installation,'machine',origin_machine,'asOf',as_of) AS state FROM fleet_feed ORDER BY position")
        .fetch_all(&mut connection).await?.iter().map(|row| row.try_get("state")).collect::<Result<_,_>>()?;
    let feed_state = query::<sqlx_turso::Turso>("SELECT json_object('id',state_id,'name',name,'head',last_position,'floor',retained_through) AS state FROM fleet_feed_state ORDER BY name")
        .fetch_all(&mut connection).await?.iter().map(|row| row.try_get("state")).collect::<Result<_,_>>()?;
    connection.close().await?;
    Ok(FleetReadback {
        records,
        rows,
        feed,
        feed_state,
    })
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn owner_fleet_import_is_atomic_and_replays_without_rewriting() -> ConformanceResult<()> {
    let jwks = JwksFixtureServer::start().await?;
    let directory = tempfile::tempdir()?;
    let server = TokioServer::start(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: directory.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let observed = async {
        let client = ConformanceClient::new(server.base_url())?;
        let bundle = fleet_bundle()?;
        let mut broken = bundle.clone();
        if let Some(fleet_admission::FleetImportRecord::Board { workspace, .. }) =
            broken.records.get_mut(1)
        {
            *workspace = message_board::WorkspaceId::try_from(
                "019f0000-0000-7000-8000-000000000599".to_owned(),
            )?;
        } else {
            return Err("fixture board missing".into());
        }
        assert!(matches!(
            client.import_fleet(&broken).await?,
            FleetImportResult::Refused { .. }
        ));
        let refused = readback(directory.path()).await?;
        assert!(refused.records.is_empty());
        assert!(refused.feed.is_empty());
        assert_eq!(
            client.import_fleet(&bundle).await?,
            FleetImportResult::Imported {
                counts: FleetImportCounts {
                    workspaces: 1,
                    boards: 1,
                    directory_entries: 1
                }
            }
        );
        let imported = readback(directory.path()).await?;
        assert_eq!(imported.records.len(), 3);
        assert_eq!(imported.feed.len(), 3);
        for record in &imported.records {
            assert_eq!(
                record.envelope().actor,
                message_board::RecordedActor::Human {
                    human_id: message_board::HumanId::try_from("importing-person".to_owned())?,
                }
            );
            assert_eq!(
                record.envelope().time,
                message_board::RecordTime::LegacyUnknown
            );
        }
        let mut relabelled = bundle.clone();
        relabelled.declared_owner = message_board::HumanId::try_from("second-importer".to_owned())?;
        relabelled.provenance.machine =
            message_board::MachineLabel::try_from("second source".to_owned())?;
        assert_eq!(
            client.import_fleet(&relabelled).await?,
            FleetImportResult::AlreadyImported
        );
        assert_eq!(readback(directory.path()).await?, imported);
        let mut conflicting = bundle.clone();
        if let Some(fleet_admission::FleetImportRecord::Workspace { description, .. }) =
            conflicting.records.first_mut()
        {
            *description = message_board::Description::try_from("Changed description".to_owned())?;
        } else {
            return Err("workspace fixture missing".into());
        }
        assert!(matches!(
            client.import_fleet(&conflicting).await?,
            FleetImportResult::Refused { .. }
        ));
        assert_eq!(readback(directory.path()).await?, imported);
        Ok(())
    }
    .await;
    let stopped = server.shutdown().await;
    let fixture_stopped = jwks.shutdown().await;
    stopped?;
    fixture_stopped?;
    observed
}
