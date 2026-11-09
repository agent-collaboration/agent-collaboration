#![cfg(feature = "conformance")]

use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, fixture_policy,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use durable_steps::storage::TransactionScope;
use fleet_admission::{FleetImportBundle, FleetImportCounts, FleetImportResult};
use sqlx_core::{query::query, row::Row};
use sqlx_turso::sqlx::{ConnectOptions, Connection};
use std::{path::Path, time::Duration};

use crate::configuration;

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
    let mut transaction = connection.begin().await?;
    let (records, rows, feed, feed_state) = {
        let mut scope = durable_steps::turso_store::TursoScope::new(
            &mut transaction,
            &crate::fleet_record_rows::ReadOnlyHost,
        );
        let records = crate::fleet_record_rows::records(&mut scope).await?;
        let mut rows = crate::fleet_record_rows::workspace_columns(&mut scope).await?;
        rows.extend(crate::fleet_record_rows::board_columns(&mut scope).await?);
        rows.extend(crate::fleet_record_rows::directory_columns(&mut scope).await?);
        let feed = crate::fleet_record_rows::feed_entries(&mut scope)
            .await?
            .into_iter()
            .map(|(_, entry)| entry)
            .collect::<Vec<_>>();
        rows.extend(crate::fleet_record_rows::feed_columns(&mut scope).await?);
        let feed_state = query::<sqlx_turso::Turso>("SELECT json_object('id',state_id,'name',name,'head',last_position,'floor',retained_through) AS state FROM fleet_feed_state ORDER BY name").fetch_all(scope.executor()).await?.into_iter().map(|row|row.try_get("state")).collect::<Result<Vec<String>,_>>()?;
        (records, rows, feed, feed_state)
    };
    transaction.commit().await?;
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
            relay_peer: fleet_protocol::PeerId::try_from(
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
