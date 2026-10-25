//! Retired owner operations retain the minimal receipt without admitting future history.
use super::{ConformanceResult, JwksFixtureServer, TokioServer, settings};
use crate::{
    copy_owner_scenarios::OwnerClient,
    imported_board_fixture::arrange_imported_unheld_board,
    owner_registry_scenarios::{complete, post},
};
use collaboration_central_conformance::{RuntimeControl, RuntimeControlRequest, owner_assertion};
use collaboration_central_core::{ConformanceBoardState, ObjectAddress, RegistryTime};
use fleet_sync_protocol::DatabaseId;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};
use std::{path::Path, time::Duration};

const DATABASE: &str = "019f0000-0000-7000-8000-000000000a83";
const RETIRED_AT: &str = "2030-03-17T17:46:40Z";

fn minimal_receipt() -> ConformanceResult<Value> {
    let at: chrono::DateTime<chrono::Utc> = RETIRED_AT.parse()?;
    Ok(json!({"kind":"retired","value":{"retiredAt":at}}))
}
async fn retire_at_literal_copy_time(base: &str) -> ConformanceResult<()> {
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    http.post(format!("{base}/admin/v1/conformance/runtime"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&RuntimeControlRequest {
            address: ObjectAddress::Copy(DatabaseId::try_from(DATABASE.to_owned())?),
            command: RuntimeControl::SetTime {
                now: RegistryTime(RETIRED_AT.parse()?),
            },
        })
        .send()
        .await?
        .error_for_status()?;
    let route = format!("{base}/admin/v1/projects/{DATABASE}/retire");
    let request = json!({"requestId":durable_steps::Uuid::now_v7(),"expectFence":1,"expectAcked":{"epoch":1,"sequence":1}});
    let initial = post(&http, &route, &request).await?;
    let retired = complete(&http, &route, &request, initial).await?;
    if retired.get("kind") != Some(&json!("retired")) {
        return Err(format!("normal owner Retire did not complete: {retired}").into());
    }
    if OwnerClient::new(base)?.health(DATABASE).await? != minimal_receipt()? {
        return Err("actual owner Retire did not store the literal Copy receipt time".into());
    }
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
struct HistoryStamp {
    corpus: String,
    version: String,
    checksum: String,
}
async fn history_stamps(root: &Path) -> ConformanceResult<Vec<HistoryStamp>> {
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("copy-{DATABASE}.db")))
        .read_only(true)
        .connect()
        .await?;
    let result = async {
        let rows = query::<sqlx_turso::Turso>(
            "SELECT corpus,version,checksum FROM central_schema_migrations ORDER BY corpus,version",
        )
        .fetch_all(&mut store)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(HistoryStamp {
                    corpus: row.try_get("corpus")?,
                    version: row.try_get("version")?,
                    checksum: row.try_get("checksum")?,
                })
            })
            .collect()
    }
    .await;
    store.close().await?;
    result
}
// Only central's own history is edited, after the owning server is stopped.
async fn make_history_unsupported(root: &Path) -> ConformanceResult<Vec<HistoryStamp>> {
    let mut expected = history_stamps(root).await?;
    let mut changed = 0_u64;
    for stamp in &mut expected {
        if stamp.corpus == "fleet-sync-receiver" && stamp.version == "3" {
            stamp.version = "999".into();
            changed += 1;
        }
    }
    if changed != 1 {
        return Err("retired fixture did not have exactly one receiver history3 stamp".into());
    }
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("copy-{DATABASE}.db")))
        .connect()
        .await?;
    let changed = query::<sqlx_turso::Turso>(
        "UPDATE central_schema_migrations SET version='999' WHERE corpus='fleet-sync-receiver' AND version='3'",
    )
    .execute(&mut store)
    .await;
    store.close().await?;
    changed?;
    if history_stamps(root).await? != expected {
        return Err("fixture changed more than the single stopped central history version".into());
    }
    Ok(expected)
}

async fn retired_owner_operations(base: &str) -> ConformanceResult<()> {
    let owner = OwnerClient::new(base)?;
    if owner.health(DATABASE).await? != minimal_receipt()? {
        return Err("reopened retired Health changed the exact minimal receipt".into());
    }
    let (status, repair) = owner.repair(DATABASE).await?;
    if status != StatusCode::GONE || repair != json!({"kind":"retired"}) {
        return Err(
            format!("retired Repair admitted unsupported bookkeeping: {status}: {repair}").into(),
        );
    }
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let migrated = post(
        &http,
        &format!("{base}/admin/v1/copies/migrate?limit=1"),
        &Value::Null,
    )
    .await?;
    if migrated
        != json!({"results":[{"project":DATABASE,"result":{"kind":"retired"}}],"nextAfter":DATABASE})
    {
        return Err(format!(
            "retired Migrate admitted or rewrote unsupported bookkeeping: {migrated}"
        )
        .into());
    }
    if owner.health(DATABASE).await? != minimal_receipt()? {
        return Err("retired Repair or Migrate changed the original minimal receipt".into());
    }
    Ok(())
}

#[tokio::test]
async fn retired_health_repair_and_migrate_keep_the_minimal_receipt_across_two_reopens()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let proof = async {
        // Native reuses canonical import and public Registry/Copy seed tasks.
        // D's real HTTP import is deliberately outside this fixture's proof.
        let database = arrange_imported_unheld_board(root.path(), ConformanceBoardState::Archived).await?;
        if database.to_string() != DATABASE {
            return Err("native retired fixture arranged a different project".into());
        }
        // Act through real owner Retire; no private receiver receipt is written.
        let first = TokioServer::start(settings(root.path(), &jwks.url)?).await?;
        let retired = retire_at_literal_copy_time(first.base_url()).await;
        let stopped = first.shutdown().await;
        retired?;
        stopped?;
        let unsupported = make_history_unsupported(root.path()).await?;
        println!("OWNER_RETIRED_REOPEN_STAGE history999_arranged passed");
        // Act and assert twice on fresh owning servers, inspecting history only at rest.
        for reopen in 1..=2 {
            let server = TokioServer::start(settings(root.path(), &jwks.url)?).await?;
            let observed = retired_owner_operations(server.base_url()).await;
            let stopped = server.shutdown().await;
            observed?;
            stopped?;
            if history_stamps(root.path()).await? != unsupported {
                return Err("retired owner operations changed the stopped history999 stamp".into());
            }
            println!("OWNER_RETIRED_REOPEN_STAGE reopen={reopen} exact_receipt_repair_migrate_and_untouched_history passed");
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let keys = jwks.shutdown().await;
    proof?;
    keys?;
    Ok(())
}
