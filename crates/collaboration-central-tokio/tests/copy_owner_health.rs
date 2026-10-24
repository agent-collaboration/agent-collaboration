//! Owner Health measures the target and stays read-only before Copy initialization.
#![cfg(feature = "conformance")]
use collaboration_central_conformance::{
    ConformanceResult, JwksFixtureServer, fixture_policy, machine_assertion, owner_assertion,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use serde_json::{Value, json};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};
use std::{path::Path, time::Duration};
#[path = "native_http/support.rs"]
mod configuration;
#[path = "support/copy_owner_scenarios.rs"]
mod copy_owner_scenarios;
#[allow(dead_code)]
#[path = "support/copy_wait_scenarios.rs"]
mod copy_wait_scenarios;
#[allow(dead_code)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;
#[allow(dead_code)]
#[path = "fleet_storage/support.rs"]
mod fleet_setup;
use fleet_setup as native_fleet;
#[allow(dead_code)]
#[path = "support/imported_board_fixture.rs"]
mod imported_board_fixture;
#[path = "copy_owner_health/migration_cases.rs"]
mod migration_cases;
#[path = "copy_owner_health/migration_reopen.rs"]
mod migration_reopen;
#[allow(dead_code)]
#[path = "support/owner_registry_scenarios.rs"]
mod owner_registry_scenarios;
#[path = "copy_owner_health/page_cases.rs"]
mod page_cases;
#[allow(dead_code)]
#[path = "mail_submit_http/registry_setup.rs"]
mod registry_setup;
#[path = "copy_owner_health/retired_reopen.rs"]
mod retired_reopen;

const PROJECT: &str = "019f0000-0000-7000-8000-000000004730";

#[path = "copy_owner_health/bookkeeping_identity.rs"]
mod bookkeeping_identity;

fn settings(root: &Path, jwks: &str) -> ConformanceResult<TokioSettings> {
    Ok(TokioSettings {
        root: root.into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.into(),
        registry: configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
}
fn measured_files(root: &Path) -> ConformanceResult<u64> {
    let mut bytes = 0_u64;
    for suffix in [".db", ".db-wal"] {
        match std::fs::metadata(root.join(format!("copy-{PROJECT}{suffix}"))) {
            Ok(metadata) => {
                bytes = bytes
                    .checked_add(metadata.len())
                    .ok_or("target file size overflow")?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(bytes)
}
fn check_live_health(body: &Value, expected_bytes: u64) -> ConformanceResult<()> {
    if body.get("kind") != Some(&json!("live")) {
        return Err(format!("fresh Copy health was not Live: {body}").into());
    }
    let value = body.get("value").ok_or("missing typed live health")?;
    if value.get("stage") != Some(&json!({"kind":"known","value":null})) {
        return Err("Health discarded the public receiver stage".into());
    }
    if value.get("copy") != Some(&json!({"kind":"known","value":null})) {
        return Err("fresh Health did not report the known absence of an initialized Copy".into());
    }
    if value.get("bookkeeping")
        != Some(
            &json!({"kind":"known","value":[{"corpus":"fleet-sync-receiver","stored":null,"supported":3}]}),
        )
    {
        return Err("Health ran Copy bookkeeping migrations before the tolerant read".into());
    }
    if value.get("migrationFailures") != Some(&json!({"kind":"known","value":[]}))
        || value.get("migrationFailure").is_some()
        || value.get("supportedBookkeepingVersion").is_some()
    {
        return Err("Health did not name its migration corpora explicitly".into());
    }
    if value.get("storageBytes").and_then(Value::as_u64) != Some(expected_bytes) {
        return Err(
            "Health did not use the target copy shell's independently measured files".into(),
        );
    }
    if value.get("lastPieceAt").is_some() {
        return Err("Health restored the dropped lastPieceAt contract".into());
    }
    Ok(())
}
#[tokio::test]
async fn owner_health_precedes_initialization_uses_target_bytes_and_refuses_machine_access()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start(settings(root.path(), &jwks.url)?).await?;
    let proof = async {
        let http = reqwest::Client::builder().no_proxy().build()?;
        let url = format!("{}/admin/v1/copies/{PROJECT}/health", server.base_url());
        let owner = owner_assertion()?;
        let response = http
            .get(&url)
            .header("Cf-Access-Jwt-Assertion", &owner)
            .send()
            .await?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(format!(
                "owner Health route is missing or blocked: {}",
                response.status()
            )
            .into());
        }
        let body = response.json::<Value>().await?;
        let measured = measured_files(root.path())?;
        if measured == 0 {
            return Err("Health never resolved a real target Copy file".into());
        }
        check_live_health(&body, measured)?;
        let response = http
            .get(format!("{url}?storage_bytes=1"))
            .header("Cf-Access-Jwt-Assertion", &owner)
            .send()
            .await?;
        if response.status() != reqwest::StatusCode::OK {
            return Err("a repeated read-only owner Health failed".into());
        }
        check_live_health(&response.json::<Value>().await?, measured)?;
        let denied = http
            .get(&url)
            .header(
                "Cf-Access-Jwt-Assertion",
                machine_assertion("copy-health-machine")?,
            )
            .send()
            .await?;
        if denied.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Err("a machine assertion reached owner Copy Health".into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    stopped?;
    keys?;
    proof?;
    // Only central's own history is observed. No private receiver query is copied.
    let mut reader = sqlx_turso::TursoConnectOptions::new()
        .filename(root.path().join(format!("copy-{PROJECT}.db")))
        .read_only(true)
        .connect()
        .await?;
    let observed=async {
        let row=query::<sqlx_turso::Turso>("SELECT count(*) AS count FROM sqlite_schema WHERE type='table' AND name IN ('central_schema_migrations','central_schema_migration_failures')").fetch_one(&mut reader).await?;
        let count:i64=row.try_get("count")?;
        if count!=0 {return Err("fresh Health created central migration bookkeeping".into());}
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    reader.close().await?;
    observed
}

#[tokio::test]
async fn owner_copy_operations_use_the_real_http_core_and_retirement_receipt()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start(settings(root.path(), &jwks.url)?).await?;
    let proof = copy_owner_scenarios::run(server.base_url()).await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    stopped?;
    keys?;
    proof
}

#[path = "copy_owner_health/migrate_cases.rs"]
mod migrate_cases;
