//! Preserve preview selection and both real retries above a Copy-only fence.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;
#[allow(dead_code)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;
#[allow(dead_code)]
#[path = "transfer_boundaries/fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/owner_registry_scenarios.rs"]
mod owner_registry_scenarios;

use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, RuntimeControl, enroll_admitted_peer,
    fixture_policy, machine_assertion,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use creation_scenarios as creation;
use durable_steps::{CallId, Uuid};
use fleet_admission::FleetOperation;
use fleet_sync_protocol::*;
use futures_util::{SinkExt, StreamExt};
use message_board::{BoardId, Description, ResourceName, WorkspaceId};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::{Value, json};
use std::time::Duration;

fn validate_fence(
    envelope: &Value,
    database: DatabaseId,
    request: &Value,
    new_epoch: u64,
    expect_epoch: u64,
) -> ConformanceResult<CallId> {
    let fence: FenceRequest = serde_json::from_value(
        envelope
            .get("request")
            .cloned()
            .ok_or("held floor-repair Fence omitted request")?,
    )?;
    if envelope.get("kind") != Some(&json!("fleet-sync.copy-fence"))
        || envelope.get("source") != Some(&json!(ObjectAddress::Registry))
        || envelope.get("destination") != Some(&json!(ObjectAddress::Copy(database)))
        || envelope.pointer("/target/copy")
            != Some(&json!(
                configuration::registry_config()?.authoritative_receiver
            ))
        || fence.database != database
        || request.get("transferId") != Some(&json!(fence.transfer_id))
        || fence.new_epoch != Epoch::new(new_epoch)?
        || fence.expect
            != (FenceExpectation {
                epoch: Epoch::new(expect_epoch)?,
                acked: fixture::position(1, 1)?,
            })
        || fence.target_schema != Some(SchemaSupport { max_version: 1 })
        || fence.holder != PeerId::try_from(fixture::TARGET_PEER.to_owned())?
    {
        return Err(format!(
            "floor-repair Fence lost exact transfer/epoch/preview/holder/schema/address: {envelope}"
        )
        .into());
    }
    Ok(serde_json::from_value(
        envelope
            .get("identity")
            .cloned()
            .ok_or("held floor Fence omitted CallId")?,
    )?)
}
async fn old_piece_is_stale(fixture: &mut fixture::Fixture, current: u64) -> ConformanceResult<()> {
    let key = "019f0000-0000-7000-8000-000000000796";
    let piece = Piece::<BoardAnnotation>::new(PieceContents {
        database: fixture.database,
        epoch: Epoch::new(1)?,
        continues_from: fixture::position(1, 1)?,
        through: fixture::position(1, 2)?,
        transactions: vec![ReplicatedTransaction::Rows {
            commit_id: CommitSequence::new(2),
            rows: vec![RowChange::upsert(
                "transfer_rows",
                vec![SqlValue::Text(key.into())],
                vec![
                    SqlValue::Text(key.into()),
                    SqlValue::Integer(1),
                    SqlValue::Text("must stay stale".into()),
                ],
            )?],
        }],
        part: None,
        annotation: None,
    })?;
    let answer = creation::operation_answer(
        &mut fixture.old,
        OperationRequest::Replication(ReplicationRequest::Piece(piece)),
    )
    .await?;
    if !matches!(answer,OperationAnswer::Replication(ReplicationAnswer::Refused(PieceRefusal::StaleEpoch {newer,ended_at})) if newer==Epoch::new(current)? && ended_at==fixture::position(1,1)?)
    {
        return Err(format!(
            "out-of-band fence lost its exact old-holder StaleEpoch{current}/cutoff1:1"
        )
        .into());
    }
    Ok(())
}
async fn finish_exact(
    fixture: &mut fixture::Fixture,
    request: &Value,
    initial: Value,
    moving: bool,
) -> ConformanceResult<()> {
    let (action, kind) = if moving {
        ("move", "moved")
    } else {
        ("takeover", "granted")
    };
    let answer = fixture.complete(action, request, initial).await?;
    let expected = json!({"kind":kind,"value":{"newEpoch":6,"start":{"epoch":1,"sequence":1}}});
    if answer != expected || fixture.post(action, request).await? != expected {
        return Err(format!(
            "out-of-band fence did not grant and replay exact {kind}6/start1:1: {answer}"
        )
        .into());
    }
    fixture.copy(6, fixture::position(1, 1)?, vec![]).await?;
    old_piece_is_stale(fixture, 6).await?;
    fixture.copy(6, fixture::position(1, 1)?, vec![]).await?;
    let start: chrono::DateTime<chrono::Utc> = "2030-03-17T17:46:40Z".parse()?;
    let expires_at = SyncTime::from_unix_millis(start.timestamp_millis() + 14_400_000);
    let renewal = creation::operation_answer(
        &mut fixture.target,
        OperationRequest::Lease(LeaseRequest::Renew(RenewRequest {
            attempt: RequestId::new(Uuid::now_v7())?,
            database: fixture.database,
            epoch: Epoch::new(6)?,
        })),
    )
    .await?;
    let expected = RenewAnswer::Renewed {
        recorded: RenewResult {
            epoch: Epoch::new(6)?,
            expires_at,
        },
        current: LeaseStatus {
            database: fixture.database,
            epoch: Epoch::new(6)?,
            holder: PeerId::try_from(fixture::TARGET_PEER.to_owned())?,
            state: LeaseState::Held,
            expires_at,
            remaining_ms: 14_400_000,
        },
        transfer_in_progress: None,
        replayed: false,
    };
    if !matches!(renewal, OperationAnswer::Lease(LeaseAnswer::Renew(actual)) if actual == expected)
    {
        return Err("floor6 grant did not install the target's exact usable Registry lease".into());
    }
    Ok(())
}
#[tokio::test]
async fn preview_reads_copy_fence_five_and_selects_six_directly() -> ConformanceResult<()> {
    let mut fixture = fixture::Fixture::open().await?;
    let mut callers = tokio::task::JoinSet::new();
    let proof = async {
        fixture.fence_five().await?;
        fixture.copy(5, fixture::position(1, 1)?, vec![]).await?;
        let preview = fixture.preview().await?;
        if preview.fence != Epoch::new(5)?
            || preview.acked != fixture::position(1, 1)?
            || preview.holder != PeerId::try_from(fixture::OLD_PEER.to_owned())?
        {
            return Err("preview selection did not read actual Copy5/start1:1/old holder".into());
        }
        println!("TRANSFER_FLOOR_STAGE actual_Preview5_before_first_Fence_selection passed");
        let request = fixture.takeover(preview, fixture::TARGET_SERVICE);
        fixture
            .control(RuntimeControl::HoldNextDispatch {
                destination: ObjectAddress::Copy(fixture.database),
            })
            .await?;
        let http = fixture.http.clone();
        let route = fixture.route("takeover");
        let sending = request.clone();
        callers.spawn(async move { owner_registry_scenarios::post(&http, &route, &sending).await });
        let held = fixture.held_copy().await?;
        let identity = validate_fence(&held, fixture.database, &request, 6, 5)?;
        if identity.step.as_str() != "fence" {
            return Err("preview selection unexpectedly required the repair retry".into());
        }
        fixture.control(RuntimeControl::ResumeDispatch).await?;
        let initial = tokio::time::timeout(Duration::from_secs(8), callers.join_next())
            .await?
            .ok_or("preview-selected Takeover caller vanished")???;
        finish_exact(&mut fixture, &request, initial, false).await?;
        println!(
            "TRANSFER_FLOOR_CASE preview_selects_first_Fence6_above_Copy5_and_exact_replay passed"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let _ = fixture.control(RuntimeControl::ResumeDispatch).await;
    callers.abort_all();
    while callers.join_next().await.is_some() {}
    let closed = fixture.close().await;
    proof?;
    closed?;
    Ok(())
}
async fn retry_above_copy_five(moving: bool) -> ConformanceResult<()> {
    let mut fixture = fixture::Fixture::open().await?;
    let mut callers = tokio::task::JoinSet::new();
    let proof=async {
        let request=if moving {json!({"transferId":Uuid::now_v7(),"toInstallation":fixture::TARGET_SERVICE,"reason":"move above out-of-band fence"})}
            else {fixture.takeover(fixture.preview().await?,fixture::TARGET_SERVICE)};
        if moving {
            let http=fixture.http.clone();let route=fixture.route("move");let sending=request.clone();
            callers.spawn(async move {owner_registry_scenarios::post(&http,&route,&sending).await});
            let (frame,_)=fixture.release(&request).await?;
            println!("TRANSFER_FLOOR_STAGE correlated_Move_Release_held_before_Copy5 passed");
            fixture.fence_five().await?;
            fixture.copy(5,fixture::position(1,1)?,vec![]).await?;
            fixture.control(RuntimeControl::HoldNextDispatch {destination:ObjectAddress::Copy(fixture.database)}).await?;
            fixture.reply_release(frame,ReleaseAnswer::Released {final_position:fixture::position(1,1)?}).await?;
            let ack=fixture.held_copy().await?;
            if ack.get("kind")!=Some(&json!("fleet-sync.copy-acknowledged")) || ack.pointer("/target/database")!=Some(&json!(fixture.database)) || ack.get("request")!=Some(&json!(AwaitAcked {through:fixture::position(1,1)?})) {return Err(format!("Move did not await its exact correlated released prefix: {ack}").into());}
            fixture.control(RuntimeControl::ResumeDispatchAndHoldNext {destination:ObjectAddress::Copy(fixture.database)}).await?;
        } else {
            fixture.fence_five().await?;
            fixture.copy(5,fixture::position(1,1)?,vec![]).await?;
            old_piece_is_stale(&mut fixture,5).await?;
            fixture.control(RuntimeControl::HoldNextDispatch {destination:ObjectAddress::Copy(fixture.database)}).await?;
            let http=fixture.http.clone();let route=fixture.route("takeover");let sending=request.clone();
            callers.spawn(async move {owner_registry_scenarios::post(&http,&route,&sending).await});
        }
        let first=fixture.held_copy().await?;
        let first_id=validate_fence(&first,fixture.database,&request,2,1)?;
        if first_id.step.as_str()!="fence" {return Err("floor repair did not first attempt original reserved Fence2".into());}
        println!("TRANSFER_FLOOR_STAGE original_Fence2_held_after_actual_Copy5 moving={moving} passed");
        fixture.control(RuntimeControl::ResumeDispatchAndHoldNext {destination:ObjectAddress::Copy(fixture.database)}).await?;
        let retry=fixture.held_copy().await?;
        let retry_id=validate_fence(&retry,fixture.database,&request,6,5)?;
        if retry_id.operation!=first_id.operation || retry_id.step.as_str()!="fence:2" {return Err("floor-repair retry lost original operation identity or exact second-Fence step".into());}
        println!("TRANSFER_FLOOR_STAGE exact_same_operation_retries_Fence6_from5 moving={moving} passed");
        fixture.control(RuntimeControl::ResumeDispatch).await?;
        let initial=tokio::time::timeout(Duration::from_secs(8),callers.join_next()).await?.ok_or("floor-repair owner caller vanished")???;
        finish_exact(&mut fixture,&request,initial,moving).await?;
        println!("TRANSFER_FLOOR_CASE actual_retry_above_Copy5_grants6_and_replays moving={moving} passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = fixture.control(RuntimeControl::ResumeDispatch).await;
    callers.abort_all();
    while callers.join_next().await.is_some() {}
    let closed = fixture.close().await;
    proof?;
    closed?;
    Ok(())
}
#[tokio::test]
async fn takeover_with_an_old_preview_retries_above_a_later_copy_fence_five()
-> ConformanceResult<()> {
    retry_above_copy_five(false).await
}
#[tokio::test]
async fn move_with_correlated_release_retries_above_a_later_copy_fence_five()
-> ConformanceResult<()> {
    retry_above_copy_five(true).await
}
