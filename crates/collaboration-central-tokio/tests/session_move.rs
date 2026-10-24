//! A normal Move retains its transfer, holder refusal and exact drained prefix.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;
#[allow(dead_code)]
#[path = "support/copy_wait_scenarios.rs"]
mod copy_wait_scenarios;
#[allow(dead_code)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;
#[path = "session_move/fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/owner_registry_scenarios.rs"]
mod owner_registry_scenarios;

use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use collaboration_central_conformance::{
    ConformanceResult, DispatchFaultObservation, RuntimeControl,
};
use collaboration_central_core::ObjectAddress;
use durable_steps::Uuid;
use fixture::{Fixture, OLD_PEER, position};
use fleet_sync_protocol::*;
use futures_util::SinkExt;
use reqwest_websocket::Message;
use serde_json::{Value, json};
use std::time::Duration;

fn move_request() -> Value {
    json!({"transferId":Uuid::now_v7(),"toInstallation":fixture::TARGET_SERVICE,"reason":"drain the normal holder"})
}

#[tokio::test]
async fn move_keeps_one_transfer_and_waits_for_the_correlated_holders_exact_final_prefix()
-> ConformanceResult<()> {
    let mut fixture = Fixture::open().await?;
    let request = move_request();
    let mut callers = tokio::task::JoinSet::new();
    fixture.start_move(&mut callers, request.clone());
    let proof = async {
        let (frame, release) = fixture.release(&request).await?;
        println!("SESSION_MOVE_STAGE correlated_holder_Release_received passed");
        let first: TransferId = serde_json::from_value(request["transferId"].clone())?;
        let conflict = fixture.post(&move_request()).await?;
        if conflict != json!({"kind":"refused","value":{"kind":"transferInProgress","value":{"transfer":first}}}) {
            return Err(format!("second Move lost the exact active transfer: {conflict}").into());
        }
        let renewal = fixture.renew_old().await?;
        if renewal != (RenewAnswer::Renewed {
            recorded: RenewResult { epoch: Epoch::new(1)?, expires_at: fixture::time(14_400_000)? },
            current: fixture::lease(fixture.database, 1, OLD_PEER)?,
            transfer_in_progress: Some(first), replayed: false,
        }) {
            return Err("renewal did not recover the exact still-active Move transfer".into());
        }
        println!("SESSION_MOVE_STAGE active_transfer_conflict_and_renewal_notice passed");
        fixture.control(RuntimeControl::HoldNextDispatch { destination:ObjectAddress::Copy(fixture.database) }).await?;
        fixture.reply(frame,ReleaseAnswer::Released {final_position:position(1,2)?}).await?;
        let DispatchFaultObservation::Held {destination,request:bytes}=fixture.control(RuntimeControl::AwaitHeldDispatch).await?.dispatch else {
            return Err("released Move did not reach its actual Copy acknowledgement call".into());
        };
        let held:Value=serde_json::from_slice(&bytes)?;
        if destination != ObjectAddress::Copy(fixture.database)
            || held.get("kind") != Some(&json!("fleet-sync.copy-acknowledged"))
            || held.pointer("/target/database") != Some(&json!(fixture.database))
            || held.get("request") != Some(&json!(AwaitAcked {through:position(1,2)?})) {
            return Err(format!("Move did not wait on its exact released database/prefix: {held}").into());
        }
        let identity:durable_steps::CallId=serde_json::from_value(held["identity"].clone())?;
        fixture.copy_unchanged(1,position(1,1)?,false).await?;
        fixture.control(RuntimeControl::ResumeDispatch).await?;
        let observer=copy_wait_scenarios::CopyClient::new(fixture.base())?;
        let observed=observer.observed(fixture.database,|state|state.waiting==1).await
            .map_err(|error|format!("released Move never owned its real below-final Copy wait: {error}"))?;
        if identity.step.as_str() != "await_acked" || observed.calls != vec![identity] || observed.attempts != 1 {
            return Err("Move Copy waiter lost its durable operation/step identity".into());
        }
        fixture.copy_unchanged(1,position(1,1)?,false).await?;
        println!("SESSION_MOVE_STAGE real_Copy_wait_below_final_prefix passed");
        fixture.apply_drained_row().await?;
        let initial=tokio::time::timeout(Duration::from_secs(8),callers.join_next()).await?
            .ok_or("normal Move caller vanished")???;
        let moved=owner_registry_scenarios::complete(&fixture.http,&fixture.route(),&request,initial).await?;
        let expected=json!({"kind":"moved","value":{"newEpoch":2,"start":{"epoch":1,"sequence":2}}});
        if moved != expected || fixture.post(&request).await? != expected || fixture.post(&request).await? != expected {
            return Err(format!("Move did not grant/replay exactly epoch2 at the drained prefix1:2: {moved}").into());
        }
        fixture.copy_unchanged(2,position(1,2)?,true).await?;
        fixture.target_renewed().await?;
        if release.epoch != Epoch::new(1)? { return Err("holder Release changed its source epoch".into()); }
        println!("SESSION_MOVE_CASE exact_catchup_transfer_notice_and_single_epoch_replay passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = fixture.control(RuntimeControl::ResumeDispatch).await;
    callers.abort_all();
    while callers.join_next().await.is_some() {}
    let closed = fixture.close().await;
    proof?;
    closed
}

async fn migration_refusal(state: MigrationState) -> ConformanceResult<()> {
    let mut fixture = Fixture::open().await?;
    let request = move_request();
    let mut callers = tokio::task::JoinSet::new();
    fixture.start_move(&mut callers, request.clone());
    let proof = async {
        let (frame, _) = fixture.release(&request).await?;
        println!("SESSION_MOVE_STAGE correlated_holder_Release_received passed");
        fixture.reply(frame, ReleaseAnswer::ReleaseRefused(ReleaseRefusal::MigrationPending { migration:2,state })).await?;
        let initial=tokio::time::timeout(Duration::from_secs(8),callers.join_next()).await?
            .ok_or("refused Move caller vanished")???;
        let refused=owner_registry_scenarios::complete(&fixture.http,&fixture.route(),&request,initial).await?;
        let expected=json!({"kind":"refused","value":{"kind":"migrationPending","value":{"migration":2,"state":state}}});
        if refused != expected || fixture.post(&request).await? != expected {
            return Err(format!("Move lost the exact holder migration refusal or its replay: {refused}; expected {expected}").into());
        }
        fixture.copy_unchanged(1,position(1,1)?,false).await?;
        let original=RenewAnswer::Renewed { recorded:RenewResult {epoch:Epoch::new(1)?,expires_at:fixture::time(14_400_000)?},current:fixture::lease(fixture.database,1,OLD_PEER)?,transfer_in_progress:None,replayed:false };
        if fixture.renew_old().await? != original { return Err("migration refusal changed the original lease or left an active transfer".into()); }
        println!("SESSION_MOVE_CASE migration_{state:?}_exact_refusal_replay_and_no_fence passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    callers.abort_all();
    while callers.join_next().await.is_some() {}
    let closed = fixture.close().await;
    proof?;
    closed
}

#[tokio::test]
async fn pending_holder_migration_is_a_typed_replayed_move_refusal_without_a_fence()
-> ConformanceResult<()> {
    migration_refusal(MigrationState::Pending).await
}
#[tokio::test]
async fn failed_holder_migration_is_a_typed_replayed_move_refusal_without_a_fence()
-> ConformanceResult<()> {
    migration_refusal(MigrationState::Failed).await
}

#[tokio::test]
async fn stale_renewal_age_preserves_its_exact_time_and_differs_from_delivery_failure()
-> ConformanceResult<()> {
    let mut fixture = Fixture::open().await?;
    let proof=async {
        creation_scenarios::pin_registry_clock(fixture.base(),collaboration_central_core::RegistryTime(
            chrono::DateTime::from_timestamp_millis(fixture::time(1_800_001)?.unix_millis()).ok_or("stale time outside range")?
        )).await?;
        println!("SESSION_MOVE_STAGE renewal_age_just_beyond_thirty_minutes passed");
        let request=move_request();
        let reply=fixture.post(&request).await?;
        let reply=owner_registry_scenarios::complete(&fixture.http,&fixture.route(),&request,reply).await?;
        let expected=json!({"kind":"refused","value":{"kind":"holderUnreachable","value":{"lastRenewedAt":fixture::time(0)?}}});
        if reply != expected || fixture.post(&request).await? != expected {return Err(format!("stale renewal age lost its exact HolderStale time/replay: {reply}").into());}
        let native:board_sync_protocol::BoardHolderChangeRefusal=serde_json::from_value(reply["value"].clone())?;
        if HolderChangeRefusal::from(native) != (HolderChangeRefusal::HolderStale {last_renewed_at:fixture::time(0)?}) {return Err("renewal observation did not decode to exact native HolderStale".into());}
        fixture.copy_unchanged(1,position(1,1)?,false).await?;
        println!("SESSION_MOVE_CASE stale_renewal_is_not_holder_unreachable passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = fixture.close().await;
    proof?;
    closed
}

#[tokio::test]
async fn a_disconnected_recent_holder_is_really_unreachable_and_keeps_the_copy_unchanged()
-> ConformanceResult<()> {
    let mut fixture = Fixture::open().await?;
    let proof=async {
        fixture.disconnect_old().await?;
        println!("SESSION_MOVE_STAGE recent_holder_socket_closed passed");
        let request=move_request();
        let reply=fixture.post(&request).await?;
        if reply != json!({"kind":"pending","value":{"transferId":request["transferId"]}}) {return Err(format!("disconnected holder did not retain its first zero-time NeverSent attempt: {reply}").into());}
        creation_scenarios::pin_registry_clock(fixture.base(),collaboration_central_core::RegistryTime(chrono::DateTime::from_timestamp_millis(fixture::time(10)?.unix_millis()).ok_or("retry time outside range")?)).await?;
        println!("SESSION_MOVE_STAGE actual_unsent_attempt_then_ten_ms_registry_advance passed");
        let reply=owner_registry_scenarios::complete(&fixture.http,&fixture.route(),&request,reply).await?;
        let expected=json!({"kind":"refused","value":{"kind":"holderUnreachable","value":{"lastSeen":null}}});
        if reply != expected || fixture.post(&request).await? != expected {return Err(format!("real delivery failure lost its exact HolderUnreachable/replay: {reply}").into());}
        let native:board_sync_protocol::BoardHolderChangeRefusal=serde_json::from_value(reply["value"].clone())?;
        if HolderChangeRefusal::from(native) != (HolderChangeRefusal::HolderUnreachable {last_seen:None}) {return Err("presence observation did not decode to exact native HolderUnreachable".into());}
        fixture.copy_from_target_unchanged().await?;
        println!("SESSION_MOVE_CASE disconnected_recent_holder_unreachable_without_fence passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = fixture.close().await;
    proof?;
    closed
}
