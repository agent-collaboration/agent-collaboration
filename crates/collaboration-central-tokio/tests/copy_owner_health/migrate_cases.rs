//! The bounded sweep visits a real canonical Fleet page and real Copy stores.
use super::{ConformanceResult, JwksFixtureServer, TokioServer, settings};
use crate::fleet_setup::{Fixture, bundle};
use collaboration_central_conformance::{machine_assertion, owner_assertion};
use serde_json::{Value, json};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    executor::Executor,
};

#[tokio::test]
async fn migrate_sweep_bounds_orders_and_continues_after_a_real_copy_failure()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let mut imported = bundle()?;
    let mut expected = vec!["019f0000-0000-7000-8000-000000000a83".to_owned()];
    for index in (0..27).rev() {
        let project = format!("019f0000-0000-7000-8000-{:012x}", 0x4750 + index);
        expected.push(project.clone());
        imported.records.push(serde_json::from_value(json!({
            "kind":"directory","project":project,
            "board":"019f0000-0000-7000-8000-000000000a82","title":format!("sweep project {index}"),
            "time":{"kind":"legacyUnknown"}
        }))?);
    }
    expected.sort();
    let imported_result = fixture.import(&imported).await;
    fixture.objects.close().await?;
    if !matches!(
        imported_result?,
        fleet_admission::FleetImportResult::Imported { .. }
    ) {
        return Err("sweep setup did not import all distinct canonical projects".into());
    }
    drop(fixture);
    // An actual first-migration collision belongs to the first directory entry.
    // Later entries have new stores, so a failed first item cannot abort the page.
    let mut bad = sqlx_turso::TursoConnectOptions::new()
        .filename(
            root.path()
                .join("copy-019f0000-0000-7000-8000-000000000a83.db"),
        )
        .create_if_missing(true)
        .connect()
        .await?;
    let migration = fleet_sync_receiver::RECEIVER_MIGRATOR
        .iter()
        .find(|migration| migration.version == 1)
        .ok_or("public receiver migration1 missing")?;
    bad.execute(migration.sql.as_ref()).await?;
    bad.close().await?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start(settings(root.path(), &jwks.url)?).await?;
    let proof = async {
        let http = reqwest::Client::builder().no_proxy().build()?;
        let route = format!("{}/admin/v1/copies/migrate", server.base_url());
        let owner = owner_assertion()?;
        for limit in [0, 26, 100] {
            let response = http
                .post(format!("{route}?limit={limit}"))
                .header("Cf-Access-Jwt-Assertion", &owner)
                .send()
                .await?;
            if response.status() != reqwest::StatusCode::BAD_REQUEST {
                return Err("Migrate silently clamped or accepted an invalid page limit".into());
            }
        }
        let denied = http
            .post(&route)
            .header(
                "Cf-Access-Jwt-Assertion",
                machine_assertion("migration-machine")?,
            )
            .send()
            .await?;
        if denied.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Err("machine reached the owner migration sweep".into());
        }
        let mut observed = Vec::new();
        let mut after = None;
        for expected_count in [25, 3, 0] {
            let page_route = after
                .as_ref()
                .map_or_else(|| route.clone(), |after| format!("{route}?after={after}"));
            let response = http
                .post(page_route)
                .header("Cf-Access-Jwt-Assertion", &owner)
                .send()
                .await?;
            let status = response.status();
            let page: Value = response.json().await?;
            if status != reqwest::StatusCode::OK {
                return Err(format!("Migrate sweep route failed: {status}: {page}").into());
            }
            let results = page
                .get("results")
                .and_then(Value::as_array)
                .ok_or("missing sweep results")?;
            if results.len() != expected_count {
                return Err(format!(
                    "Migrate page ignored its actual max25 or exclusive cursor: {page}"
                )
                .into());
            }
            for item in results {
                let project = item
                    .get("project")
                    .and_then(Value::as_str)
                    .ok_or("result has no project")?;
                let result = item
                    .get("result")
                    .ok_or("project lost its migration result")?;
                if project == expected[0] {
                    if result.get("kind") != Some(&json!("failed"))
                        || result.get("version") != Some(&Value::Null)
                        || result.get("corpus") != Some(&json!("fleet-sync-receiver"))
                    {
                        return Err(
                            format!("Migrate hid its actual failed first Copy: {item}").into()
                        );
                    }
                } else if !matches!(
                    result.get("kind").and_then(Value::as_str),
                    Some("migrated" | "current")
                ) {
                    return Err(
                        format!("Migrate aborted or fabricated a later result: {item}").into(),
                    );
                }
                observed.push(project.to_owned());
            }
            after = page
                .get("nextAfter")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if after.as_deref()
                != results
                    .last()
                    .and_then(|item| item.get("project"))
                    .and_then(Value::as_str)
            {
                return Err("Migrate cursor did not name the page's last actual result".into());
            }
        }
        if observed != expected {
            return Err(
                format!("Migrate skipped, repeated or misordered a Copy: {observed:?}").into(),
            );
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}
