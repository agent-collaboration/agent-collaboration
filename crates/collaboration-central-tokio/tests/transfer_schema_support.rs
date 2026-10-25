//! Enrollment absence, unknown support and holder schema refusals stay distinct.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;
#[allow(dead_code)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;
#[allow(dead_code)]
#[path = "transfer_boundaries/fixture.rs"]
mod fixture;
#[path = "transfer_boundaries/grant_readback.rs"]
mod grant_readback;
#[path = "transfer_missing_copy/mod.rs"]
mod missing_copy;
#[allow(dead_code)]
#[path = "support/owner_registry_scenarios.rs"]
mod owner_registry_scenarios;

use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, RuntimeControl, enroll_admitted_peer,
    enroll_open_peer_with_label, fixture_policy, machine_assertion,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use creation_scenarios as creation;
use durable_steps::Uuid;
use fleet_admission::FleetOperation;
use fleet_sync_protocol::*;
use futures_util::{SinkExt, StreamExt};
use message_board::{BoardId, Description, ResourceName, WorkspaceId};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::{Value, json};
use std::time::Duration;

async fn close_and_check(
    fixture: fixture::Fixture,
    proof: ConformanceResult<()>,
) -> ConformanceResult<()> {
    let database = fixture.database;
    let root = fixture.close().await?;
    proof?;
    grant_readback::original_only(root.path(), database).await?;
    println!("TRANSFER_SUPPORT_STAGE public_stopped_history_original_grant_only passed");
    Ok(())
}
#[tokio::test]
async fn known_target_hello_zero_refuses_the_actual_copy_schema_without_a_grant()
-> ConformanceResult<()> {
    let mut fixture = fixture::Fixture::open().await?;
    let proof=async {
        fixture.target_schema(0).await?;
        let schema=fixture.unchanged().await?;
        let request=fixture.takeover(fixture.preview().await?,fixture::TARGET_SERVICE);
        let initial=fixture.post("takeover",&request).await?;
        let refused=fixture.complete("takeover",&request,initial).await?;
        let expected=json!({"kind":"refused","value":{"kind":"schemaTooNew","value":{"store":schema,"supported":0}}});
        if refused!=expected || fixture.post("takeover",&request).await?!=expected {return Err(format!("known target max0 lost its exact schema refusal/replay: {refused}").into());}
        fixture.unchanged().await?;
        println!("TRANSFER_SUPPORT_CASE known_Hello0_exact_schema_refusal_no_Copy_change passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    close_and_check(fixture, proof).await
}
#[tokio::test]
async fn never_enrolled_target_is_not_found_before_admission_without_a_grant()
-> ConformanceResult<()> {
    let mut fixture = fixture::Fixture::open().await?;
    let proof = async {
        let request = fixture.takeover(fixture.preview().await?, &Uuid::now_v7().to_string());
        let refused = fixture.post("takeover", &request).await?;
        let expected = json!({"kind":"refused","value":{"kind":"notFound"}});
        if refused != expected || fixture.post("takeover", &request).await? != expected {
            return Err(format!(
                "never-enrolled target lost exact pre-admission NotFound: {refused}"
            )
            .into());
        }
        fixture.unchanged().await?;
        println!(
            "TRANSFER_SUPPORT_CASE never_enrolled_NotFound_exact_repeat_no_Copy_change passed"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    close_and_check(fixture, proof).await
}
#[tokio::test]
async fn enrolled_target_without_hello_reaches_a_real_fence_with_unknown_schema_support()
-> ConformanceResult<()> {
    let mut fixture = fixture::Fixture::open().await?;
    let unknown = enroll_open_peer_with_label(
        &ConformanceClient::new(fixture.base())?,
        "550e8400-e29b-41d4-a716-446655440794",
        "019f0000-0000-7000-8000-000000000794",
        "known enrollment without Hello",
        "no schema report",
        "019f0000-0000-7000-8000-000000000795",
    )
    .await?;
    let mut callers = tokio::task::JoinSet::new();
    let proof=async {
        let schema=fixture.unchanged().await?;
        let request=fixture.takeover(fixture.preview().await?,"550e8400-e29b-41d4-a716-446655440794");
        fixture.control(RuntimeControl::HoldNextDispatch {destination:ObjectAddress::Copy(fixture.database)}).await?;
        let http=fixture.http.clone();let route=fixture.route("takeover");let sending=request.clone();
        callers.spawn(async move {owner_registry_scenarios::post(&http,&route,&sending).await});
        let held=fixture.held_copy().await?;
        let fence:FenceRequest=serde_json::from_value(held.get("request").cloned().ok_or("unknown-support Fence omitted request")?)?;
        if held.get("kind")!=Some(&json!("fleet-sync.copy-fence")) || fence.database!=fixture.database
            || request.get("transferId")!=Some(&json!(fence.transfer_id)) || fence.new_epoch!=Epoch::new(2)?
            || fence.holder!=PeerId::try_from("019f0000-0000-7000-8000-000000000794".to_owned())?
            || fence.expect!=(FenceExpectation {epoch:Epoch::new(1)?,acked:fixture::position(1,1)?}) || fence.target_schema.is_some() {
            return Err(format!("known/no-Hello producer did not reach its exact real Fence with supportNone: {held}").into());
        }
        fixture.control(RuntimeControl::ResumeDispatch).await?;
        let initial=tokio::time::timeout(Duration::from_secs(8),callers.join_next()).await?.ok_or("unknown-support Takeover caller vanished")???;
        let refused=fixture.complete("takeover",&request,initial).await?;
        let expected=json!({"kind":"refused","value":{"kind":"schemaTooNew","value":{"store":schema,"supported":null}}});
        if refused!=expected || fixture.post("takeover",&request).await?!=expected {return Err(format!("known/no-Hello target lost exact SchemaTooNew(None)/replay: {refused}").into());}
        fixture.unchanged().await?;
        println!("TRANSFER_SUPPORT_CASE known_no_Hello_actual_Fence_supportNone_exact_schema_refusal passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = fixture.control(RuntimeControl::ResumeDispatch).await;
    callers.abort_all();
    while callers.join_next().await.is_some() {}
    let unknown_closed = unknown
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let result = close_and_check(fixture, proof).await;
    unknown_closed?;
    result
}
#[tokio::test]
async fn correlated_holder_schema_refusal_is_preserved_in_move_and_its_replay()
-> ConformanceResult<()> {
    let mut fixture = fixture::Fixture::open().await?;
    let mut callers = tokio::task::JoinSet::new();
    let proof=async {
        fixture.target_schema(0).await?;
        let schema=fixture.unchanged().await?;
        let request=json!({"transferId":Uuid::now_v7(),"toInstallation":fixture::TARGET_SERVICE,"reason":"schema refusal on release"});
        let http=fixture.http.clone();let route=fixture.route("move");let sending=request.clone();
        callers.spawn(async move {owner_registry_scenarios::post(&http,&route,&sending).await});
        let (frame,_)=fixture.release(&request).await?;
        println!("TRANSFER_SUPPORT_STAGE exact_correlated_holder_Release_before_schema_refusal passed");
        fixture.reply_release(frame,ReleaseAnswer::ReleaseRefused(ReleaseRefusal::SchemaTooNew {store:schema.clone(),supported:Some(0)})).await?;
        let initial=tokio::time::timeout(Duration::from_secs(8),callers.join_next()).await?.ok_or("schema-refused Move caller vanished")???;
        let refused=fixture.complete("move",&request,initial).await?;
        let expected=json!({"kind":"refused","value":{"kind":"schemaTooNew","value":{"store":schema,"supported":0}}});
        if refused!=expected || fixture.post("move",&request).await?!=expected {return Err(format!("Move erased its exact correlated holder schema refusal/replay: {refused}").into());}
        fixture.unchanged().await?;
        println!("TRANSFER_SUPPORT_CASE correlated_holder_schema_refusal_and_admin_replay_no_fence passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    callers.abort_all();
    while callers.join_next().await.is_some() {}
    close_and_check(fixture, proof).await
}
