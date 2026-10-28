//! Real native migration rollback, independent metadata reads and recovery.
use super::{ConformanceResult, JwksFixtureServer, TokioServer, settings};
use crate::copy_owner_scenarios::{OwnerClient, bookkeeping, no_failures};
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    raw_sql::raw_sql,
    sql_str::AssertSqlSafe,
};
use std::{future::Future, path::Path};

async fn sql(root: &Path, database: &str, statements: &[&'static str]) -> ConformanceResult<()> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("copy-{database}.db")))
        .connect()
        .await?;
    let result = async {
        for statement in statements {
            raw_sql(AssertSqlSafe(*statement))
                .execute(&mut connection)
                .await?;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    connection.close().await?;
    result
}
async fn serve<Scenario, Run>(root: &Path, jwks: &str, scenario: Scenario) -> ConformanceResult<()>
where
    Scenario: FnOnce(String) -> Run,
    Run: Future<Output = ConformanceResult<()>>,
{
    let server = TokioServer::start(settings(root, jwks)?).await?;
    let observed = scenario(server.base_url().to_owned()).await;
    server.shutdown().await?;
    observed
}
fn pending(status: StatusCode, body: &Value, version: u64) -> ConformanceResult<()> {
    let expected = json!({"kind":"copyUnavailable","reason":{"kind":"migrationPending","value":{"version":version}}});
    if status != StatusCode::SERVICE_UNAVAILABLE || body != &expected {
        return Err(
            format!("migration failure lost its original typed 503: {status}: {body}").into(),
        );
    }
    Ok(())
}
fn failures(health: &Value, expected: &[(&str, u64, &str)]) -> ConformanceResult<()> {
    let rows = health
        .pointer("/value/migrationFailures/value")
        .and_then(Value::as_array)
        .ok_or("Health did not return readable migration failures")?;
    if rows.len() != expected.len() {
        return Err(
            format!("Health lost or invented a current migration failure: {health}").into(),
        );
    }
    for (corpus, version, reason) in expected {
        let row = rows
            .iter()
            .find(|row| row.get("corpus") == Some(&json!(corpus)))
            .ok_or_else(|| format!("Health omitted failed corpus {corpus}: {health}"))?;
        if row.get("version") != Some(&json!(version))
            || !row
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|detail| detail.contains(reason))
            || row.get("at").and_then(Value::as_str).is_none()
        {
            return Err(format!(
                "Health lost the actual failed corpus/version/cause/time: {health}"
            )
            .into());
        }
    }
    Ok(())
}

#[tokio::test]
async fn receiver_failure_survives_rollback_best_effort_recording_and_clears_on_success()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let database = "019f0000-0000-7000-8000-000000004733";
    let proof = async {
        serve(root.path(), &jwks.url, |base| async move {
            let client = OwnerClient::new(&base)?;
            let (status, body) = client.get(&format!("/admin/v1/copies/{database}/health")).await?;
            if status != StatusCode::OK {return Err(format!("initial owner operation failed: {status}: {body}").into());}
            bookkeeping(&body, None)
        }).await?;
        // Both fixtures affect central-owned metadata. No receiver-private table is queried.
        sql(root.path(), database, &[
            include_str!("../../../collaboration-central-core/core_migrations/0000_migration_history.sql"),
            include_str!("../../../collaboration-central-core/core_migrations/0009_migration_failures.sql"),
            "CREATE TRIGGER receiver_history_fault BEFORE INSERT ON central_schema_migrations WHEN NEW.corpus='fleet-sync-receiver' AND NEW.version='3' BEGIN SELECT RAISE(ABORT,'receiver history fault'); END",
            "CREATE TRIGGER diagnostic_write_fault BEFORE INSERT ON central_schema_migration_failures BEGIN SELECT RAISE(ABORT,'failure record fault'); END",
        ]).await?;
        serve(root.path(), &jwks.url, |base| async move {
            let client = OwnerClient::new(&base)?;
            let (status, body) = client.repair(database).await?;
            pending(status, &body, 3)?;
            let health = client.health(database).await?;
            bookkeeping(&health, None)?;
            no_failures(&health)
        }).await?;
        sql(root.path(), database, &["DROP TRIGGER diagnostic_write_fault"]).await?;
        serve(root.path(), &jwks.url, |base| async move {
            let client = OwnerClient::new(&base)?;
            let (status, body) = client.repair(database).await?;
            pending(status, &body, 3)?;
            let health = client.health(database).await?;
            bookkeeping(&health, None)?;
            failures(&health, &[("fleet-sync-receiver", 3, "receiver history fault")])
        }).await?;
        sql(root.path(), database, &[
            "DROP TRIGGER receiver_history_fault",
            "CREATE TABLE _copy_summary (fixture TEXT)",
        ]).await?;
        serve(root.path(), &jwks.url, |base| async move {
            let client = OwnerClient::new(&base)?;
            let (status, body) = client.repair(database).await?;
            pending(status, &body, 1)?;
            let health = client.health(database).await?;
            bookkeeping(&health, None)?;
            // The hook error rolls back the receiver's earlier failure clear too.
            failures(&health, &[
                ("fleet-sync-receiver", 3, "receiver history fault"),
                ("central-copy-summary", 1, "_copy_summary"),
            ])
        }).await?;
        sql(root.path(), database, &["DROP TABLE _copy_summary"]).await?;
        serve(root.path(), &jwks.url, |base| async move {
            let client = OwnerClient::new(&base)?;
            let (status, body) = client.repair(database).await?;
            if status != StatusCode::OK {return Err(format!("recovered receiver Repair failed: {status}: {body}").into());}
            let health = client.health(database).await?;
            bookkeeping(&health, Some(3))?;
            no_failures(&health)
        }).await
    }.await;
    jwks.shutdown().await?;
    proof
}

#[tokio::test]
async fn hook_failure_names_its_own_corpus_and_success_clears_it() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let database = "019f0000-0000-7000-8000-000000004734";
    let proof = async {
        serve(root.path(), &jwks.url, |base| async move {
            let client = OwnerClient::new(&base)?;
            bookkeeping(&client.health(database).await?, None)
        })
        .await?;
        // Collision with central's own hook DDL forces an actual initialize_hook error.
        sql(
            root.path(),
            database,
            &["CREATE TABLE _copy_summary (fixture TEXT)"],
        )
        .await?;
        serve(root.path(), &jwks.url, |base| async move {
            let client = OwnerClient::new(&base)?;
            let (status, body) = client.repair(database).await?;
            pending(status, &body, 1)?;
            let health = client.health(database).await?;
            // All receiver migrations preceded the hook failure in the rolled-back step.
            bookkeeping(&health, None)?;
            failures(&health, &[("central-copy-summary", 1, "_copy_summary")])
        })
        .await?;
        sql(root.path(), database, &["DROP TABLE _copy_summary"]).await?;
        serve(root.path(), &jwks.url, |base| async move {
            let client = OwnerClient::new(&base)?;
            let (status, body) = client.repair(database).await?;
            if status != StatusCode::OK {
                return Err(format!("recovered hook Repair failed: {status}: {body}").into());
            }
            let health = client.health(database).await?;
            bookkeeping(&health, Some(3))?;
            no_failures(&health)
        })
        .await
    }
    .await;
    jwks.shutdown().await?;
    proof
}

#[tokio::test]
async fn health_reads_history_independently_of_an_unreadable_failure_table() -> ConformanceResult<()>
{
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let database = "019f0000-0000-7000-8000-000000004735";
    let proof = async {
        serve(root.path(), &jwks.url, |base| async move {
            let client = OwnerClient::new(&base)?;
            let (status, body) = client.repair(database).await?;
            if status != StatusCode::OK {
                return Err(format!("initial owner operation failed: {status}: {body}").into());
            }
            Ok(())
        })
        .await?;
        sql(
            root.path(),
            database,
            &[
                "DROP TABLE central_schema_migration_failures",
                "CREATE TABLE central_schema_migration_failures (wrong_shape TEXT)",
            ],
        )
        .await?;
        serve(root.path(), &jwks.url, |base| async move {
            let client = OwnerClient::new(&base)?;
            let health = client.health(database).await?;
            bookkeeping(&health, Some(3))?;
            if health.pointer("/value/migrationFailures/kind") != Some(&json!("unreadable"))
                || health
                    .pointer("/value/migrationFailures/value/detail")
                    .and_then(Value::as_str)
                    .is_none()
            {
                return Err(format!("Health hid an unreadable failure table: {health}").into());
            }
            Ok(())
        })
        .await
    }
    .await;
    jwks.shutdown().await?;
    proof
}
