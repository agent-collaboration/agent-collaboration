//! One normal Fleet/Registry/Copy path, reused by Tokio and workerd.
use super::creation_scenarios as creation;
use board_sync_protocol::BoardConsumerProtocol;
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer, owner_assertion,
};
use durable_steps::Uuid;
use fleet_admission::FleetOperation;
use fleet_sync_protocol::*;
use futures_util::{SinkExt, StreamExt};
use message_board::{BoardId, Description, ResourceName, WorkspaceId};
use reqwest_websocket::{Message, WebSocket};
use serde_json::{Value, json};
use std::time::Duration;
const OLD_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440591";
const OLD_PEER: &str = "019f0000-0000-7000-8000-000000000591";
const NEW_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440592";
const NEW_PEER: &str = "019f0000-0000-7000-8000-000000000592";

pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base)?;
    let mut old = enroll_admitted_peer(
        &client,
        OLD_SERVICE,
        OLD_PEER,
        "owner-old-holder",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let mut target = enroll_admitted_peer(
        &client,
        NEW_SERVICE,
        NEW_PEER,
        "owner-new-holder",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let mut calls = tokio::task::JoinSet::new();
    let proof=async {
        creation::pin_registry_clock(base,collaboration_central_core::RegistryTime("2030-03-17T17:46:40Z".parse()?)).await?;
        let workspace=WorkspaceId::generate();let board=BoardId::generate();let database=DatabaseId::new(Uuid::now_v7())?;
        for operation in [
            FleetOperation::WorkspaceCreate {name:ResourceName::try_from("owner admin workspace".to_owned())?,description:Description::try_from("normal owner admin proof".to_owned())?},
            FleetOperation::BoardCreate {board:board.clone(),name:ResourceName::try_from("owner admin board".to_owned())?,description:Description::try_from("real Fleet archive gate".to_owned())?,board_kind:message_board::BoardKind::Area {}},
            creation::project(database,board.clone())?,
        ] {creation::accepted(creation::metadata(&mut old.socket,&workspace,operation).await?)?;}
        creation::registered(&creation::register(&mut old.socket,database,PeerId::try_from(OLD_PEER.to_owned())?,RequestId::new(Uuid::now_v7())?).await?,PeerId::try_from(OLD_PEER.to_owned())?)?;
        let page=http.get(format!("{base}/admin/v1/copies?limit=1")).header("Cf-Access-Jwt-Assertion",owner_assertion()?).send().await?.error_for_status()?.json::<Value>().await?;
        if page.pointer("/projects/0/project")!=Some(&json!(database)) || page.get("nextAfter")!=Some(&json!(database)) {return Err(format!("normal registered project missing from owner Page: {page}").into());}
        let migrate=post(&http,&format!("{base}/admin/v1/copies/migrate?limit=1"),&Value::Null).await?;
        if migrate != json!({"results":[{"project":database,"result":{"kind":"current","corpus":"fleet-sync-receiver","version":3}}],"nextAfter":database}) {return Err(format!("Migrate lost its normal registered Copy: {migrate}").into());}
        println!("OWNER_REGISTRY_CASE normal_fleet_creation_page_and_migrate passed");
        let route=format!("{base}/admin/v1/projects/{database}");
        let moving=json!({"transferId":Uuid::now_v7(),"toInstallation":NEW_SERVICE,"reason":"normal owner move"});
        let send_http=http.clone();let send_route=format!("{route}/move");let send_body=moving.clone();
        calls.spawn(async move {post(&send_http,&send_route,&send_body).await});
        let (frame,release)=release(&mut old.socket).await?;
        if release.database!=database || release.epoch.value()!=1 || moving.get("transferId")!=Some(&serde_json::to_value(release.transfer)?) {return Err("normal owner Move sent the wrong holder Release".into());}
        old.socket.send(Message::Binary(serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Answer {
            frame,body:ClientAnswer::Crate(ServerCallAnswer::Release(ReleaseAnswer::Released {final_position:position(1,0)?})),
        })?.into())).await?;
        let answer=tokio::time::timeout(Duration::from_secs(8),calls.join_next()).await?.ok_or("normal owner Move caller vanished")???;
        let moved=complete(&http,&format!("{route}/move"),&moving,answer).await?;
        grant(&moved,"moved",2)?;
        if post(&http,&format!("{route}/move"),&moving).await?!=moved {return Err("normal owner Move did not replay its committed answer".into());}
        match creation::operation_answer(&mut target.socket,OperationRequest::Lease(LeaseRequest::Renew(RenewRequest {attempt:RequestId::new(Uuid::now_v7())?,database,epoch:Epoch::new(2)?}))).await? {
            OperationAnswer::Lease(LeaseAnswer::Renew(RenewAnswer::Renewed {..}))=>{},
            _=>return Err("owner Move did not install a usable target grant".into()),
        }
        println!("OWNER_REGISTRY_CASE normal_move_release_fence_target_renewal_and_replay passed");
        let preview=post(&http,&format!("{route}/takeover/preview"),&Value::Null).await?;
        if preview.get("holderInstallation")!=Some(&json!(NEW_SERVICE)) {return Err("normal takeover preview confused installation with peer".into());}
        let native:TakeoverPreview=serde_json::from_value(preview.get("preview").cloned().ok_or("normal preview omitted native confirmation")?)?;
        if native.holder!=PeerId::try_from(NEW_PEER.to_owned())? || native.fence.value()!=2 {return Err("normal preview did not read the moved holder/fence".into());}
        let mut tampered=serde_json::to_value(&native)?;*tampered.get_mut("acked").ok_or("native preview omitted acked")?=json!({"epoch":1,"sequence":1});
        let refused=post(&http,&format!("{route}/takeover"),&json!({"transferId":Uuid::now_v7(),"toInstallation":OLD_SERVICE,"reason":"tampered preview","preview":tampered})).await?;
        if refused!=json!({"kind":"refused","value":{"kind":"previewStale"}}) {return Err("normal takeover accepted an altered native preview".into());}
        let taking=json!({"transferId":Uuid::now_v7(),"toInstallation":OLD_SERVICE,"reason":"owner confirms native preview","preview":native});
        let granted=post(&http,&format!("{route}/takeover"),&taking).await?;
        let granted=complete(&http,&format!("{route}/takeover"),&taking,granted).await?;
        grant(&granted,"granted",3)?;
        if post(&http,&format!("{route}/takeover"),&taking).await?!=granted {return Err("normal takeover did not replay its exact grant".into());}
        let retired=post(&http,&format!("{route}/retire"),&json!({"requestId":Uuid::now_v7(),"expectFence":3,"expectAcked":{"epoch":1,"sequence":0}})).await?;
        if retired!=json!({"kind":"refused","value":{"kind":"leaseHeld"}}) {return Err(format!("normal owner retire lost its actual held-lease refusal: {retired}").into());}
        println!("OWNER_REGISTRY_CASE native_preview_tamper_takeover_replay_and_lease_held passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    calls.abort_all();
    while calls.join_next().await.is_some() {}
    let closed_old = old
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let closed_target = target
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed_old?;
    closed_target?;
    Ok(())
}
fn position(epoch: u64, sequence: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(epoch)?,
        CommitSequence::new(sequence),
    ))
}
fn grant(value: &Value, kind: &str, epoch: u64) -> ConformanceResult<()> {
    if value != &json!({"kind":kind,"value":{"newEpoch":epoch,"start":{"epoch":1,"sequence":0}}}) {
        return Err(format!("normal owner holder change lost its exact grant: {value}").into());
    }
    Ok(())
}
pub(super) async fn post(
    http: &reqwest::Client,
    route: &str,
    body: &Value,
) -> ConformanceResult<Value> {
    let response = http
        .post(route)
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(body)
        .send()
        .await?;
    let status = response.status();
    let value = response.json::<Value>().await?;
    if status != reqwest::StatusCode::OK {
        return Err(format!("owner HTTP {route}: {status}: {value}").into());
    }
    Ok(value)
}
pub(super) async fn complete(
    http: &reqwest::Client,
    route: &str,
    request: &Value,
    initial: Value,
) -> ConformanceResult<Value> {
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut value = initial;
        while value.get("kind") == Some(&json!("pending")) {
            value = post(http, route, request).await?;
        }
        Ok(value)
    })
    .await?
}
async fn release(socket: &mut WebSocket) -> ConformanceResult<(FrameId, ReleaseRequest)> {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or("normal owner holder closed before Release")??;
            let bytes = match message {
                Message::Binary(bytes) => bytes.to_vec(),
                Message::Text(text) => text.into_bytes(),
                _ => continue,
            };
            match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
                ServerFrame::Request {
                    frame,
                    body: ServerRequest::Crate(ServerCall::Release { request, .. }),
                } => return Ok((frame, request)),
                ServerFrame::Notice { .. } => {}
                _ => return Err("normal owner holder received an unexpected frame".into()),
            }
        }
    })
    .await?
}

/// The only arranged state is public import+seed; the action is normal owner HTTP.
pub(super) async fn retire_imported_fixture(base: &str) -> ConformanceResult<()> {
    let database = DatabaseId::try_from("019f0000-0000-7000-8000-000000005596".to_owned())?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let arranged=post(&http,&format!("{base}/admin/v1/conformance/bootstrap"),&json!({"fixture":"importedUnheld","command":{"kind":"importedUnheld","value":{"database":database,"boardState":collaboration_central_core::ConformanceBoardState::Archived}}})).await?;
    if arranged != json!({"epoch":1,"sequence":1}) {
        return Err(format!(
            "composed import fixture did not publish its seeded prefix: {arranged}"
        )
        .into());
    }
    let client = ConformanceClient::new(base)?;
    let mut reader = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-446655440597",
        "019f0000-0000-7000-8000-000000005597",
        "owner retirement readback",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let proof=async {
        match creation::operation_answer(&mut reader.socket,OperationRequest::Replication(ReplicationRequest::SnapshotPage {database,cursor:None})).await? {
            OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page)))
                if page.table=="owner_retirement_rows" && page.rows==vec![vec![SqlValue::Text("019f0000-0000-7000-8000-000000005595".into()),SqlValue::Text("fixture imported row".into())]]=>{},
            _=>return Err("normal reader did not observe the fixture's actual seeded row before retirement".into()),
        }
        let route=format!("{base}/admin/v1/projects/{database}/retire");
        let request=json!({"requestId":Uuid::now_v7(),"expectFence":1,"expectAcked":{"epoch":1,"sequence":1}});
        let first=post(&http,&route,&request).await?;
        let retired=complete(&http,&route,&request,first).await?;
        if retired.get("kind")!=Some(&json!("retired")) {return Err(format!("normal imported-project retirement did not finish: {retired}").into());}
        let at:SyncTime=serde_json::from_value(retired.pointer("/value/at").cloned().ok_or("owner Retired omitted its exact native time")?)?;
        let page=http.get(format!("{base}/admin/v1/copies?limit=100")).header("Cf-Access-Jwt-Assertion",owner_assertion()?).send().await?.error_for_status()?.json::<Value>().await?;
        let projected=page.get("projects").and_then(Value::as_array).and_then(|projects|projects.iter().find(|project|project.get("project")==Some(&json!(database)))).ok_or("retired project disappeared from directory")?;
        let projected_at=chrono::DateTime::from_timestamp_millis(at.unix_millis()).ok_or("retired time outside fixture display range")?;
        if projected.get("leaseSide")!=Some(&json!({"kind":"known","value":{"lease":{"kind":"retired","value":{"retired_at":projected_at}},"asOf":projected.get("leaseSide").and_then(|side|side.pointer("/value/asOf")).ok_or("retirement projection omitted observation time")?}})) {
            return Err(format!("HTTP Retired escaped before the actual exact Fleet projection: {projected}").into());
        }
        let health_route=format!("{base}/admin/v1/copies/{database}/health");
        let health=http.get(&health_route).header("Cf-Access-Jwt-Assertion",owner_assertion()?).send().await?.error_for_status()?.json::<Value>().await?;
        // Copy records its wipe time; Registry records its later lifecycle
        // completion. Each keeps its own native time and stable replay.
        let copy_at: chrono::DateTime<chrono::Utc> = health.pointer("/value/retiredAt").and_then(Value::as_str).ok_or("Copy receipt omitted its own native wipe time")?.parse()?;
        if health!=json!({"kind":"retired","value":{"retiredAt":copy_at}}) {return Err(format!("owner retirement did not leave the minimal native Copy receipt: {health}").into());}
        if !matches!(creation::operation_answer(&mut reader.socket,OperationRequest::Replication(ReplicationRequest::SnapshotPage {database,cursor:None})).await?,OperationAnswer::Replication(ReplicationAnswer::Refused(PieceRefusal::Retired))) {return Err("retired Copy still exposed its formerly seeded rows".into());}
        if post(&http,&route,&request).await?!=retired {return Err("owner retirement replay changed its native time".into());}
        if post(&http,&format!("{base}/admin/v1/projects/{database}/takeover/preview"),&Value::Null).await?!=json!({"kind":"retired"}) {return Err("retired preview fabricated a live holder".into());}
        let migrate=post(&http,&format!("{base}/admin/v1/copies/migrate?limit=25"),&Value::Null).await?;
        let item=migrate.get("results").and_then(Value::as_array).and_then(|results|results.iter().find(|item|item.get("project")==Some(&json!(database)))).ok_or("Migrate omitted retired project")?;
        if item.get("result")!=Some(&json!({"kind":"retired"})) {return Err("Migrate reinitialized a retired fixture Copy".into());}
        if http.get(&health_route).header("Cf-Access-Jwt-Assertion",owner_assertion()?).send().await?.error_for_status()?.json::<Value>().await?!=health {return Err("Migrate or replay restamped the retired Copy receipt".into());}
        println!("OWNER_RETIREMENT_CASE fixture_imported_unheld_seed_normal_retire_exact_directory_and_receipt_replay passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = reader
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}
