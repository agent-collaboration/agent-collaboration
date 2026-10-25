//! The owner HTTP surface uses the same Copy and Fleet core on both shells.
use super::copy_wait_scenarios::CopyClient;
use collaboration_central_conformance::{ConformanceResult, machine_assertion, owner_assertion};
use fleet_sync_protocol::{DatabaseId, Epoch};
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

pub(super) struct OwnerClient {
    http: reqwest::Client,
    base: String,
    assertion: String,
}
impl OwnerClient {
    pub(super) fn new(base: &str) -> ConformanceResult<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(10))
                .build()?,
            base: base.into(),
            assertion: owner_assertion()?,
        })
    }
    pub(super) async fn get(&self, route: &str) -> ConformanceResult<(StatusCode, Value)> {
        let response = self
            .http
            .get(format!("{}{route}", self.base))
            .header("Cf-Access-Jwt-Assertion", &self.assertion)
            .send()
            .await?;
        Ok((response.status(), response.json().await?))
    }
    pub(super) async fn repair(&self, database: &str) -> ConformanceResult<(StatusCode, Value)> {
        let response = self
            .http
            .post(format!("{}/admin/v1/copies/{database}/repair", self.base))
            .header("Cf-Access-Jwt-Assertion", &self.assertion)
            .send()
            .await?;
        Ok((response.status(), response.json().await?))
    }
    pub(super) async fn health(&self, database: &str) -> ConformanceResult<Value> {
        let (status, body) = self
            .get(&format!("/admin/v1/copies/{database}/health"))
            .await?;
        if status != StatusCode::OK {
            return Err(format!("Health failed {status}: {body}").into());
        }
        Ok(body)
    }
}
pub(super) fn bookkeeping(health: &Value, stored: Option<u64>) -> ConformanceResult<()> {
    let expected = json!({"kind":"known","value":[{"corpus":"fleet-sync-receiver","stored":stored,"supported":3}]});
    if health.pointer("/value/bookkeeping") != Some(&expected) {
        return Err(format!("Health did not report the actual receiver corpus: {health}").into());
    }
    if health.pointer("/value/migrationFailure").is_some()
        || health
            .pointer("/value/supportedBookkeepingVersion")
            .is_some()
    {
        return Err("Health retained an ambiguous scalar migration field".into());
    }
    Ok(())
}
pub(super) fn no_failures(health: &Value) -> ConformanceResult<()> {
    if health.pointer("/value/migrationFailures") != Some(&json!({"kind":"known","value":[]})) {
        return Err(format!("Health retained a migration failure after success: {health}").into());
    }
    Ok(())
}
pub async fn run(base: &str) -> ConformanceResult<()> {
    let client = OwnerClient::new(base)?;
    let fresh = "019f0000-0000-7000-8000-000000004731";
    let first = client.health(fresh).await?;
    bookkeeping(&first, None)?;
    no_failures(&first)?;
    if first.pointer("/value/stage") != Some(&json!({"kind":"known","value":null}))
        || first.pointer("/value/archived").is_some()
        || first.pointer("/value/lastPieceAt").is_some()
    {
        return Err("fresh Health lost its stage or restored a dead Copy field".into());
    }
    if first
        .pointer("/value/storageBytes")
        .and_then(Value::as_u64)
        .filter(|bytes| *bytes > 0)
        .is_none()
    {
        return Err("Health did not measure a real target store".into());
    }
    let (status, forged) = client
        .get(&format!("/admin/v1/copies/{fresh}/health?storage_bytes=1"))
        .await?;
    if status != StatusCode::OK
        || forged.pointer("/value/storageBytes") != first.pointer("/value/storageBytes")
    {
        return Err("a caller replaced the target shell's storage measurement".into());
    }
    let (status, repaired) = client.repair(fresh).await?;
    if status != StatusCode::OK || repaired != json!({"kind":"repaired"}) {
        return Err(format!(
            "Repair did not use the ordinary Copy prelude/alarm path: {status}: {repaired}"
        )
        .into());
    }
    let repaired = client.health(fresh).await?;
    bookkeeping(&repaired, Some(3))?;
    no_failures(&repaired)?;
    if repaired.pointer("/value/copy") != Some(&json!({"kind":"known","value":null})) {
        return Err("Repair invented an initialized replicated Copy".into());
    }
    let (status, page) = client.get("/admin/v1/copies?limit=1").await?;
    if status != StatusCode::OK
        || page.get("projects") != Some(&json!([]))
        || page.get("nextAfter") != Some(&Value::Null)
    {
        return Err(format!("empty owner directory page was wrong: {status}: {page}").into());
    }
    for limit in [0, 101] {
        let (status, body) = client
            .get(&format!("/admin/v1/copies?limit={limit}"))
            .await?;
        if status != StatusCode::BAD_REQUEST || body != json!({"kind":"invalidPageLimit"}) {
            return Err("owner page admitted a limit outside 1..=100".into());
        }
    }
    let machine = machine_assertion("copy-owner-machine")?;
    for route in [
        format!("/admin/v1/copies/{fresh}/health"),
        "/admin/v1/copies".into(),
    ] {
        let response = client
            .http
            .get(format!("{base}{route}"))
            .header("Cf-Access-Jwt-Assertion", &machine)
            .send()
            .await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return Err("machine reached owner Copy operations".into());
        }
    }
    let response = client
        .http
        .post(format!("{base}/admin/v1/copies/{fresh}/repair"))
        .header("Cf-Access-Jwt-Assertion", machine)
        .send()
        .await?;
    if response.status() != StatusCode::UNAUTHORIZED {
        return Err("machine reached owner Repair".into());
    }

    let retired = "019f0000-0000-7000-8000-000000004732";
    let database = DatabaseId::try_from(retired.to_owned())?;
    let copy = CopyClient::new(base)?;
    copy.create(database).await?;
    let initialized = client.health(retired).await?;
    if initialized.pointer("/value/stage") != Some(&json!({"kind":"known","value":"following"}))
        || initialized.pointer("/value/copy/value/database") != Some(&json!(database))
        || initialized.pointer("/value/copy/value/fence") != Some(&json!(1))
        || initialized.pointer("/value/copy/value/acked") != Some(&json!({"epoch":1,"sequence":0}))
    {
        return Err(
            format!("Health lost the actual receiver stage/status facts: {initialized}").into(),
        );
    }
    copy.retire(database, Epoch::new(1)?).await?;
    let receipt = client.health(retired).await?;
    if receipt.get("kind") != Some(&json!("retired"))
        || receipt
            .pointer("/value/retiredAt")
            .and_then(Value::as_str)
            .is_none()
        || receipt.get("transfer").is_some()
    {
        return Err(format!("Health lost the minimal native retirement receipt: {receipt}").into());
    }
    let (status, body) = client.repair(retired).await?;
    if status != StatusCode::GONE
        || body != json!({"kind":"retired"})
        || client.health(retired).await? != receipt
    {
        return Err("Repair changed or re-stamped a retired Copy receipt".into());
    }
    println!("COPY_OWNER_CASE health_page_repair_authority_and_retired_receipt passed");
    Ok(())
}
