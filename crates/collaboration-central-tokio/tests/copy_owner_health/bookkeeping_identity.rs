//! Tolerant owner Health rejects physical identity corruption without repairing it.
use super::{ConformanceResult, JwksFixtureServer, TokioServer, settings};
use crate::copy_owner_scenarios::{OwnerClient, bookkeeping};
use serde_json::Value;
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};

type HistoryRows = Vec<(String, String, String, String)>;

async fn history(connection: &mut sqlx_turso::TursoConnection) -> ConformanceResult<HistoryRows> {
    query::<sqlx_turso::Turso>(
        "SELECT row_id,corpus,version,checksum FROM central_schema_migrations ORDER BY corpus,version",
    )
    .fetch_all(connection)
    .await?
    .into_iter()
    .map(|row| Ok((row.try_get("row_id")?, row.try_get("corpus")?, row.try_get("version")?, row.try_get("checksum")?)))
    .collect()
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

async fn read_health_with_identity(invalid_id: Option<&str>) -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start_conformance(settings(root.path(), &jwks.url)?).await?;
    let database = "019f0000-0000-7000-8000-000000001048";
    let proof = async {
        let owner = OwnerClient::new(server.base_url())?;
        let (status, answer) = owner.repair(database).await?;
        if status != reqwest::StatusCode::OK {
            return Err(format!("real Copy bookkeeping setup failed: {status}: {answer}").into());
        }
        let mut reader = sqlx_turso::TursoConnectOptions::new()
            .filename(root.path().join(format!("copy-{database}.db")))
            .connect().await?;
        let observed = async {
            let healthy = history(&mut reader).await?;
            let original = healthy.iter().find(|row| row.1 == "fleet-sync-receiver" && row.2 == "3")
                .ok_or("real receiver migration stamp missing")?;
            let original_id = durable_steps::Uuid::parse_str(&original.0)?;
            if original_id.get_version_num() != 7 {
                return Err("healthy migration stamp is not UUIDv7".into());
            }
            if let Some(id) = invalid_id {
                let updated = query::<sqlx_turso::Turso>(
                    "UPDATE central_schema_migrations SET row_id=? WHERE corpus='fleet-sync-receiver' AND version='3'",
                ).bind(id).execute(&mut reader).await?;
                if updated.rows_affected() != 1 { return Err("identity fixture did not replace one stamp".into()); }
            }
            let before = history(&mut reader).await?;
            let files_before = stored_bytes(root.path(), database)?;
            let health = owner.health(database).await?;
            let unchanged = history(&mut reader).await?;
            if before != unchanged || files_before != stored_bytes(root.path(), database)? {
                return Err("Health committed a mutation while reading bookkeeping".into());
            }
            if invalid_id.is_some() {
                let section = health.pointer("/value/bookkeeping").ok_or("bookkeeping section missing")?;
                if section.get("kind").and_then(Value::as_str) != Some("unreadable")
                    || !section.pointer("/value/detail").and_then(Value::as_str).is_some_and(|detail| detail.contains("migration row identity")) {
                    return Err(format!("Health accepted the invalid physical migration identity: {section}").into());
                }
                if health.pointer("/value/copy/kind").and_then(Value::as_str) != Some("known") {
                    return Err("invalid bookkeeping hid an independently readable Copy section".into());
                }
            } else {
                bookkeeping(&health, Some(3))?;
            }
            Ok(())
        }.await;
        reader.close().await?;
        observed
    }.await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}

#[tokio::test]
async fn healthy_history_identity_keeps_health_known_without_writing() -> ConformanceResult<()> {
    read_health_with_identity(None).await
}

#[tokio::test]
async fn malformed_history_identity_makes_health_unreadable_without_writing()
-> ConformanceResult<()> {
    read_health_with_identity(Some("malformed-migration-row")).await
}

#[tokio::test]
async fn uuid_v4_history_identity_makes_health_unreadable_without_writing() -> ConformanceResult<()>
{
    read_health_with_identity(Some("550e8400-e29b-41d4-a716-446655440000")).await
}
