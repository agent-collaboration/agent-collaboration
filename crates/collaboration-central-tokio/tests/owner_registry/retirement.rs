//! Retirement checks the real Fleet archive, wipes the real Copy and publishes.
use super::*;
use fleet_admission::{
    DirectoryLease, DirectorySide, FleetImportRecord, FleetImportResult, ImportedBoardState,
    LeaseMetadata,
};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};

const DATABASE: &str = "019f0000-0000-7000-8000-000000000a83";

async fn arrange(root: &Path, archived: bool) -> ConformanceResult<()> {
    let fleet = native_fleet::Fixture::open(root).await?;
    let mut bundle = native_fleet::bundle()?;
    if archived {
        for record in &mut bundle.records {
            if let FleetImportRecord::Board { state, .. } = record {
                *state = ImportedBoardState::Archived;
            }
        }
    }
    let imported = fleet.import(&bundle).await;
    fleet.objects.close().await?;
    if !matches!(imported?, FleetImportResult::Imported { .. }) {
        return Err("retirement setup did not import its canonical Fleet graph".into());
    }
    drop(fleet);
    registry_setup::import_seeded(
        root,
        configuration::registry_config()?,
        DatabaseId::try_from(DATABASE.to_owned())?,
    )
    .await?;
    if project_rows(root).await? != 1 {
        return Err("retirement setup did not publish the real seeded project row".into());
    }
    Ok(())
}
fn request() -> Value {
    json!({"requestId":durable_steps::Uuid::now_v7(),"expectFence":1,"expectAcked":{"epoch":1,"sequence":1}})
}
async fn project_rows(root: &Path) -> ConformanceResult<i64> {
    // This project table and its one literal row are fixture-authored, not
    // receiver-private schema. Read only after the owning runtime has closed.
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("copy-{DATABASE}.db")))
        .read_only(true)
        .connect()
        .await?;
    let count = query::<sqlx_turso::Turso>("SELECT COUNT(*) AS count FROM claim_rows")
        .fetch_one(&mut connection)
        .await;
    connection.close().await?;
    Ok(count?.try_get("count")?)
}
async fn finish(http: &reqwest::Client, route: &str, body: &Value) -> ConformanceResult<Value> {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let reply = fixture::post(http, route, body).await?;
            if reply.get("kind") != Some(&json!("pending")) {
                return Ok(reply);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?
}

#[tokio::test]
async fn owner_retirement_refuses_an_open_board_without_wiping_its_copy() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    arrange(root.path(), false).await?;
    let keys = JwksFixtureServer::start().await?;
    let server = support::start(root.path(), &keys.url).await?;
    let proof = async {
        let http = reqwest::Client::builder().no_proxy().build()?;
        let reply = finish(
            &http,
            &format!("{}/admin/v1/projects/{DATABASE}/retire", server.base_url()),
            &request(),
        )
        .await?;
        if reply.get("kind") != Some(&json!("refused"))
            || !matches!(
                serde_json::from_value::<RetirementRefusal>(
                    reply
                        .get("value")
                        .cloned()
                        .ok_or("retirement refusal lost its native payload")?
                )?,
                RetirementRefusal::GateRefused { .. }
            )
        {
            return Err(format!(
                "open board was not refused by the real Fleet retirement gate: {reply}"
            )
            .into());
        }
        let health = http
            .get(format!(
                "{}/admin/v1/copies/{DATABASE}/health",
                server.base_url()
            ))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        if health.get("kind") != Some(&json!("live")) {
            return Err("refused retirement wiped the real Copy".into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    server.shutdown().await?;
    keys.shutdown().await?;
    proof?;
    if project_rows(root.path()).await? != 1 {
        return Err("refused open-board retirement deleted a replicated row".into());
    }
    Ok(())
}

#[tokio::test]
async fn owner_retirement_publishes_retired_then_replays_the_original_time_after_reopen()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    arrange(root.path(), true).await?;
    let keys = JwksFixtureServer::start().await?;
    let http = reqwest::Client::builder().no_proxy().build()?;
    let offered = request();
    let server = support::start(root.path(), &keys.url).await?;
    let proof=async {
        let route=format!("{}/admin/v1/projects/{DATABASE}/retire",server.base_url());
        let retired=finish(&http,&route,&offered).await?;
        if retired.get("kind")!=Some(&json!("retired")) {return Err(format!("archived unheld project was not retired: {retired}").into());}
        let at:SyncTime=serde_json::from_value(retired.pointer("/value/at").cloned().ok_or("retirement lost its native time")?)?;
        let page=http.get(format!("{}/admin/v1/copies?limit=100",server.base_url()))
            .header("Cf-Access-Jwt-Assertion",owner_assertion()?).send().await?.error_for_status()?.json::<Value>().await?;
        let entries:Vec<DirectoryEntry>=serde_json::from_value(page.get("projects").cloned().ok_or("owner Page omitted projects")?)?;
        let entry=entries.iter().find(|entry|entry.project.as_str()==DATABASE).ok_or("retired project disappeared from Page")?;
        if !matches!(&entry.lease_side,DirectorySide::Known(LeaseMetadata {lease:DirectoryLease::Retired {retired_at},..})
            if retired_at.timestamp_millis()==at.unix_millis()) {
            return Err("HTTP Retired was reported before the real directory acknowledged its exact retirement".into());
        }
        let health=http.get(format!("{}/admin/v1/copies/{DATABASE}/health",server.base_url()))
            .header("Cf-Access-Jwt-Assertion",owner_assertion()?).send().await?.error_for_status()?.json::<Value>().await?;
        if health.get("kind")!=Some(&json!("retired")) {return Err("successful retirement left a live Copy".into());}
        if finish(&http,&route,&offered).await?!=retired {return Err("same-ID retirement restamped its outcome".into());}
        let mut conflicting=offered.clone();conflicting["expectFence"]=json!(2);
        if fixture::post(&http,&route,&conflicting).await?.get("kind")!=Some(&json!("conflictingRequest")) {
            return Err("retirement accepted different contents under its request ID".into());
        }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>((retired,health))
    }.await;
    server.shutdown().await?;
    let (retired, health) = match proof {
        Ok(value) => value,
        Err(error) => {
            keys.shutdown().await?;
            return Err(error);
        }
    };
    if project_rows(root.path()).await? != 0 {
        keys.shutdown().await?;
        return Err("successful owner retirement retained replicated project data".into());
    }
    let reopened = support::start(root.path(), &keys.url).await?;
    let proof = async {
        let route = format!(
            "{}/admin/v1/projects/{DATABASE}/retire",
            reopened.base_url()
        );
        if finish(&http, &route, &offered).await? != retired {
            return Err("reopened retirement did not replay its original library time".into());
        }
        let repeated = http
            .get(format!(
                "{}/admin/v1/copies/{DATABASE}/health",
                reopened.base_url()
            ))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        if repeated != health {
            return Err("reopened retirement changed the minimal Copy receipt".into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    reopened.shutdown().await?;
    keys.shutdown().await?;
    proof
}

#[tokio::test]
async fn owner_retirement_preserves_the_native_lease_held_refusal() -> ConformanceResult<()> {
    let fixture = Fixture::open().await?;
    let proof=async {
        let body=json!({"requestId":durable_steps::Uuid::now_v7(),"expectFence":1,"expectAcked":{"epoch":1,"sequence":0}});
        let reply=fixture.post("retire",&body).await?;
        if reply.get("kind")!=Some(&json!("refused")) || serde_json::from_value::<RetirementRefusal>(
            reply.get("value").cloned().ok_or("lease-held refusal lost its native payload")?)?!=RetirementRefusal::LeaseHeld {
            return Err(format!("held lease did not prevent owner retirement: {reply}").into());
        }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.close().await?;
    proof
}
