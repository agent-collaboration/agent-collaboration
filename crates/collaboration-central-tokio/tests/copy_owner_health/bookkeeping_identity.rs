//! Tolerant owner Health rejects a receiver migration record outside sqlx's
//! shape without repairing it (PD1).
use super::{ConformanceResult, JwksFixtureServer, TokioServer, settings};
use crate::copy_owner_health::copy_owner_scenarios::{OwnerClient, bookkeeping, receiver_current};
use serde_json::Value;
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};

type HistoryRows = Vec<(i64, String, i64, Vec<u8>)>;

async fn history(connection: &mut sqlx_turso::TursoConnection) -> ConformanceResult<HistoryRows> {
    query::<sqlx_turso::Turso>(
        "SELECT CAST(version AS INTEGER) AS version,CAST(installed_on AS TEXT) AS installed_on,CAST(success AS INTEGER) AS success,checksum FROM _sqlx_migrations_fleet_sync_receiver ORDER BY version",
    )
    .fetch_all(connection)
    .await?
    .into_iter()
    .map(|row| Ok((row.try_get("version")?, row.try_get("installed_on")?, row.try_get("success")?, row.try_get("checksum")?)))
    .collect()
}

/// A corruption of the receiver's version-3 record, and the field Health must name.
struct RecordCorruption {
    update: &'static str,
    field: &'static str,
}

fn stored_bytes(root: &std::path::Path, database: &str) -> ConformanceResult<Vec<Option<Vec<u8>>>> {
    [".db", ".db-wal"]
        .into_iter()
        .map(
            |suffix| match std::fs::read(root.join(format!("copy-{database}{suffix}"))) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error.into()),
            },
        )
        .collect()
}

async fn read_health_with_record(corruption: Option<RecordCorruption>) -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start(settings(root.path(), &jwks.url)?).await?;
    let database = "019f0000-0000-7000-8000-000000001048";
    let proof = async {
        let owner = OwnerClient::new(server.base_url())?;
        let (status, answer) = owner.repair(database).await?;
        if status != reqwest::StatusCode::OK {
            return Err(format!("real Copy bookkeeping setup failed: {status}: {answer}").into());
        }
        let mut reader = sqlx_turso::TursoConnectOptions::new()
            .filename(root.path().join(format!("copy-{database}.db")))
            .connect()
            .await?;
        let observed = async {
            let healthy = history(&mut reader).await?;
            let original = healthy
                .iter()
                .find(|row| row.0 == 3)
                .ok_or("real receiver migration record missing")?;
            if original.2 != 1 || original.3.len() != 48 {
                return Err("healthy receiver record is not sqlx's shape".into());
            }
            if let Some(corruption) = &corruption {
                let updated = query::<sqlx_turso::Turso>(corruption.update)
                    .execute(&mut reader)
                    .await?;
                if updated.rows_affected() != 1 {
                    return Err("corruption fixture did not change one record".into());
                }
            }
            let before = history(&mut reader).await?;
            let files_before = stored_bytes(root.path(), database)?;
            let health = owner.health(database).await?;
            let unchanged = history(&mut reader).await?;
            if before != unchanged || files_before != stored_bytes(root.path(), database)? {
                return Err("Health committed a mutation while reading bookkeeping".into());
            }
            if let Some(corruption) = &corruption {
                let section = health
                    .pointer("/value/bookkeeping")
                    .ok_or("bookkeeping section missing")?;
                if section.get("kind").and_then(Value::as_str) != Some("unreadable")
                    || !section
                        .pointer("/value/detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| detail.contains(corruption.field))
                {
                    return Err(format!(
                        "Health accepted a receiver record outside sqlx's shape: {section}"
                    )
                    .into());
                }
                if health.pointer("/value/copy/kind").and_then(Value::as_str) != Some("known") {
                    return Err(
                        "invalid bookkeeping hid an independently readable Copy section".into(),
                    );
                }
            } else {
                bookkeeping(&health, Some(receiver_current()))?;
            }
            Ok(())
        }
        .await;
        reader.close().await?;
        observed
    }
    .await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}

#[tokio::test]
async fn a_healthy_receiver_record_keeps_health_known_without_writing() -> ConformanceResult<()> {
    read_health_with_record(None).await
}

#[tokio::test]
async fn a_receiver_record_with_a_non_timestamp_install_time_makes_health_unreadable_without_writing()
-> ConformanceResult<()> {
    read_health_with_record(Some(RecordCorruption {
        update: "UPDATE _sqlx_migrations_fleet_sync_receiver SET installed_on='yesterday' WHERE version=3",
        field: "_sqlx_migrations_fleet_sync_receiver.installed_on",
    }))
    .await
}

#[tokio::test]
async fn a_failed_receiver_record_makes_health_unreadable_without_writing() -> ConformanceResult<()>
{
    read_health_with_record(Some(RecordCorruption {
        update: "UPDATE _sqlx_migrations_fleet_sync_receiver SET success=0 WHERE version=3",
        field: "_sqlx_migrations_fleet_sync_receiver.success",
    }))
    .await
}
