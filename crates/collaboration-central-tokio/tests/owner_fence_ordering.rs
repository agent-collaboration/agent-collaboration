//! Both real owner Fence/Piece orders preserve one complete acknowledged prefix.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;
#[allow(dead_code)]
#[path = "support/creation_scenarios.rs"]
mod creation;

use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, DispatchFaultObservation, JwksFixtureServer,
    RuntimeControl, RuntimeControlRequest, RuntimeFaultObservation, enroll_admitted_peer,
    fixture_policy, owner_assertion,
};
use collaboration_central_core::ObjectAddress;
use collaboration_central_tokio::{TokioServer, TokioSettings};
use durable_steps::{CallTarget, Uuid};
use fleet_admission::FleetOperation;
use fleet_sync_protocol::*;
use futures_util::StreamExt;
use message_board::{BoardId, Description, ResourceName, ServiceId, WorkspaceId};
use reqwest_websocket::{Message, WebSocket};
use serde_json::{Value, json};
use std::time::Duration;

const OLD_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440741";
const OLD_PEER: &str = "019f0000-0000-7000-8000-000000000741";
const NEW_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440742";
const NEW_PEER: &str = "019f0000-0000-7000-8000-000000000742";
const FIRST_KEY: &str = "019f0000-0000-7000-8000-000000000743";
const SECOND_KEY: &str = "019f0000-0000-7000-8000-000000000744";

#[derive(Clone, Copy)]
enum FirstCall {
    Piece,
    Fence,
}
#[derive(Clone, Copy)]
enum ReleaseOrder {
    PieceFirst,
    FenceFirst,
    Together,
}

fn original_position(sequence: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(1)?,
        CommitSequence::new(sequence),
    ))
}
fn normal_piece(database: DatabaseId, schema: bool) -> ConformanceResult<Piece<BoardAnnotation>> {
    let transactions = if schema {
        vec![ReplicatedTransaction::SchemaStep {
            commit_id: CommitSequence::new(1), migration_version: 1,
            statements: vec!["CREATE TABLE records(record_key TEXT NOT NULL PRIMARY KEY,id INTEGER NOT NULL,label TEXT NOT NULL)".into()],
        }]
    } else {
        vec![ReplicatedTransaction::Rows {
            commit_id: CommitSequence::new(2),
            rows: vec![
                RowChange::upsert(
                    "records",
                    vec![SqlValue::Text(FIRST_KEY.into())],
                    vec![
                        SqlValue::Text(FIRST_KEY.into()),
                        SqlValue::Integer(1),
                        SqlValue::Text("overlap-1".into()),
                    ],
                )?,
                RowChange::upsert(
                    "records",
                    vec![SqlValue::Text(SECOND_KEY.into())],
                    vec![
                        SqlValue::Text(SECOND_KEY.into()),
                        SqlValue::Integer(2),
                        SqlValue::Text("overlap-2".into()),
                    ],
                )?,
            ],
        }]
    };
    Ok(Piece::new(PieceContents {
        database,
        epoch: Epoch::new(1)?,
        continues_from: original_position(if schema { 0 } else { 1 })?,
        through: original_position(if schema { 1 } else { 2 })?,
        transactions,
        part: None,
        annotation: None,
    })?)
}
async fn runtime_control(
    base: &str,
    address: ObjectAddress,
    command: RuntimeControl,
) -> ConformanceResult<RuntimeFaultObservation> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!("{base}/admin/v1/conformance/runtime"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&RuntimeControlRequest { address, command })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
async fn held_request(
    base: &str,
    source: ObjectAddress,
    destination: &ObjectAddress,
) -> ConformanceResult<Value> {
    let observation = runtime_control(base, source, RuntimeControl::AwaitHeldDispatch).await?;
    let DispatchFaultObservation::Held {
        destination: actual,
        request,
    } = observation.dispatch
    else {
        return Err("expected dispatch was never held".into());
    };
    if &actual != destination {
        return Err("held dispatch addresses another physical Copy".into());
    }
    Ok(serde_json::from_slice(&request)?)
}
async fn owner_post(http: &reqwest::Client, route: &str, body: &Value) -> ConformanceResult<Value> {
    Ok(http
        .post(route)
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
async fn finish_takeover(
    http: &reqwest::Client,
    route: &str,
    body: &Value,
    initial: Value,
) -> ConformanceResult<Value> {
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut answer = initial;
        while answer.get("kind") == Some(&json!("pending")) {
            answer = owner_post(http, route, body).await?;
        }
        Ok(answer)
    })
    .await?
}
async fn piece_answer(
    socket: &mut WebSocket,
    expected: FrameId,
) -> ConformanceResult<ReplicationAnswer> {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or("old holder ended before the held Piece answer")??;
            let bytes = match message {
                Message::Binary(bytes) => bytes.to_vec(),
                Message::Text(text) => text.into_bytes(),
                _ => continue,
            };
            match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
                ServerFrame::Notice { .. } => {}
                ServerFrame::Answer {
                    frame,
                    body: OperationAnswer::Replication(answer),
                } if frame == expected => return Ok(answer),
                _ => return Err("held Piece answer lost its original frame correlation".into()),
            }
        }
    })
    .await?
}
async fn assert_copy(
    socket: &mut WebSocket,
    database: DatabaseId,
    first: FirstCall,
) -> ConformanceResult<()> {
    let (fence, holder, sequence, rows) = match first {
        FirstCall::Piece => (
            1,
            OLD_PEER,
            2,
            vec![
                vec![
                    SqlValue::Text(FIRST_KEY.into()),
                    SqlValue::Integer(1),
                    SqlValue::Text("overlap-1".into()),
                ],
                vec![
                    SqlValue::Text(SECOND_KEY.into()),
                    SqlValue::Integer(2),
                    SqlValue::Text("overlap-2".into()),
                ],
            ],
        ),
        FirstCall::Fence => (2, NEW_PEER, 1, vec![]),
    };
    let answer = creation::operation_answer(
        socket,
        OperationRequest::Replication(ReplicationRequest::CopyStatus { database }),
    )
    .await?;
    if !matches!(answer,OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))) if status.database==database && status.role==CopyRole::Authoritative && status.fence==Epoch::new(fence)? && status.holder==Some(PeerId::try_from(holder.to_owned())?) && status.acked==original_position(sequence)? && status.schema.version==1)
    {
        return Err(
            "Fence/Piece outcome mixed its Copy holder, fence, schema or committed prefix".into(),
        );
    }
    let answer = creation::operation_answer(
        socket,
        OperationRequest::Replication(ReplicationRequest::SnapshotPage {
            database,
            cursor: None,
        }),
    )
    .await?;
    if !matches!(answer,OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page))) if page.database==database && page.position==original_position(sequence)? && page.schema.version==1 && page.table=="records" && page.rows==rows && page.next.is_none())
    {
        return Err(
            "Fence/Piece outcome mixed a complete prefix with partial or unexpected rows".into(),
        );
    }
    Ok(())
}

async fn normal_held_order(base: &str, order: ReleaseOrder) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base)?;
    let mut old = enroll_admitted_peer(
        &client,
        OLD_SERVICE,
        OLD_PEER,
        "ordering-old",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let mut target = enroll_admitted_peer(
        &client,
        NEW_SERVICE,
        NEW_PEER,
        "ordering-target",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let session = ObjectAddress::Connection(ServiceId::try_from(OLD_SERVICE.to_owned())?);
    let mut callers = tokio::task::JoinSet::new();
    let proof=async {
        // Arrange normal Fleet parents, creation, Register and a real empty schema1 Copy.
        let database=DatabaseId::new(Uuid::now_v7())?;
        let copy=ObjectAddress::Copy(database);
        let workspace=WorkspaceId::generate();let board=BoardId::generate();
        for operation in [
            FleetOperation::WorkspaceCreate {name:ResourceName::try_from("owner ordering workspace".to_owned())?,description:Description::try_from("normal Fence and Piece ordering".to_owned())?},
            FleetOperation::BoardCreate {board:board.clone(),name:ResourceName::try_from("owner ordering board".to_owned())?,description:Description::try_from("actual complete-prefix oracle".to_owned())?,board_kind:message_board::BoardKind::Area {}},
            creation::project(database,board)?,
        ] {creation::accepted(creation::metadata(&mut old.socket,&workspace,operation).await?)?;}
        creation::registered(&creation::register(&mut old.socket,database,PeerId::try_from(OLD_PEER.to_owned())?,RequestId::new(Uuid::now_v7())?).await?,PeerId::try_from(OLD_PEER.to_owned())?)?;
        if !matches!(creation::operation_answer(&mut old.socket,OperationRequest::Replication(ReplicationRequest::Piece(normal_piece(database,true)?))).await?,OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through==original_position(1)?) {return Err("ordering fixture did not commit its real initial schema".into());}
        let route=format!("{base}/admin/v1/projects/{database}/takeover");
        let preview=owner_post(&http,&format!("{route}/preview"),&Value::Null).await?;
        let preview:TakeoverPreview=serde_json::from_value(preview.get("preview").cloned().ok_or("ordering Preview omitted its native confirmation")?)?;
        if preview.fence!=Epoch::new(1)? || preview.acked!=original_position(1)? || preview.holder!=PeerId::try_from(OLD_PEER.to_owned())? {return Err("ordering Preview was not the real old-holder prefix".into());}
        let transfer=TransferId::new(Uuid::now_v7())?;
        let confirmation=json!({"transferId":transfer,"toInstallation":NEW_SERVICE,"reason":"forced real Fence/Piece ordering","preview":preview});
        // Act: hold the actual Registry Fence before creating the exact held Piece.
        runtime_control(base,ObjectAddress::Registry,RuntimeControl::HoldNextDispatch {destination:copy.clone()}).await?;
        let send_http=http.clone();let send_route=route.clone();let send_body=confirmation.clone();
        callers.spawn(async move {owner_post(&send_http,&send_route,&send_body).await});
        let fence_envelope=held_request(base,ObjectAddress::Registry,&copy).await?;
        if fence_envelope.get("kind")!=Some(&json!(<fleet_sync_lease::CopyFence as CallTarget>::KIND)) {return Err("owner Takeover held another call instead of its real Fence".into());}
        let fence:FenceRequest=serde_json::from_value(fence_envelope.get("request").cloned().ok_or("held Fence omitted its native request")?)?;
        if fence.database!=database || fence.transfer_id!=transfer || fence.new_epoch!=Epoch::new(2)? || fence.expect!= (FenceExpectation {epoch:Epoch::new(1)?,acked:original_position(1)?}) || fence.holder!=PeerId::try_from(NEW_PEER.to_owned())? {return Err("held Fence belongs to another transfer, holder or preview prefix".into());}
        runtime_control(base,session.clone(),RuntimeControl::HoldNextDispatch {destination:copy.clone()}).await?;
        let piece=normal_piece(database,false)?;
        let frame=creation::send(&mut old.socket,OperationRequest::Replication(ReplicationRequest::Piece(piece.clone()))).await?;
        let piece_envelope=held_request(base,session.clone(),&copy).await?;
        let held:Piece<BoardAnnotation>=serde_json::from_value(piece_envelope.pointer("/request/request/value").cloned().ok_or("held session request omitted its exact Piece")?)?;
        if piece_envelope.get("kind")!=Some(&json!("central.copy-session-write")) || piece_envelope.pointer("/request/sender")!=Some(&json!(OLD_PEER)) || held!=piece {return Err("the held session dispatch is not this exact old-holder Piece/digest/prefix".into());}
        // Both holds must still exist together; setup completion is not overlap proof.
        if !matches!(runtime_control(base,ObjectAddress::Registry,RuntimeControl::Observe).await?.dispatch,DispatchFaultObservation::Held {..}) || !matches!(runtime_control(base,session.clone(),RuntimeControl::Observe).await?.dispatch,DispatchFaultObservation::Held {..}) {return Err("Fence and exact Piece were not simultaneously pending before release".into());}
        println!("OWNER_FENCE_ORDERING_STAGE both_correlated_calls_held passed");
        let initial;
        let piece_result;
        match order {
            ReleaseOrder::PieceFirst=>{
                runtime_control(base,session.clone(),RuntimeControl::ResumeDispatch).await?;
                piece_result=piece_answer(&mut old.socket,frame).await?;
                runtime_control(base,ObjectAddress::Registry,RuntimeControl::ResumeDispatch).await?;
                initial=tokio::time::timeout(Duration::from_secs(8),callers.join_next()).await?.ok_or("Takeover HTTP caller disappeared")???;
            },
            ReleaseOrder::FenceFirst=>{
                runtime_control(base,ObjectAddress::Registry,RuntimeControl::ResumeDispatch).await?;
                initial=tokio::time::timeout(Duration::from_secs(8),callers.join_next()).await?.ok_or("Takeover HTTP caller disappeared")???;
                // Observe actual completion before releasing the other real call.
                let completed=finish_takeover(&http,&route,&confirmation,initial.clone()).await?;
                if completed!=json!({"kind":"granted","value":{"newEpoch":2,"start":{"epoch":1,"sequence":1}}}) {return Err(format!("Fence-first owner Takeover did not grant the exact original prefix: {completed}").into());}
                runtime_control(base,session.clone(),RuntimeControl::ResumeDispatch).await?;
                piece_result=piece_answer(&mut old.socket,frame).await?;
            },
            ReleaseOrder::Together=>{
                let (resumed_fence,resumed_piece)=tokio::join!(
                    runtime_control(base,ObjectAddress::Registry,RuntimeControl::ResumeDispatch),
                    runtime_control(base,session.clone(),RuntimeControl::ResumeDispatch),
                );
                resumed_fence?;resumed_piece?;
                println!("OWNER_FENCE_ORDERING_STAGE both_resumed_before_answers passed");
                let (http_result,answer_result)=tokio::join!(
                    async { tokio::time::timeout(Duration::from_secs(8),callers.join_next()).await?.ok_or("Takeover HTTP caller disappeared")?? },
                    piece_answer(&mut old.socket,frame),
                );
                initial=http_result?;
                piece_result=answer_result?;
            },
        }
        let takeover=finish_takeover(&http,&route,&confirmation,initial).await?;
        let first=match order {
            ReleaseOrder::PieceFirst=>FirstCall::Piece,
            ReleaseOrder::FenceFirst=>FirstCall::Fence,
            ReleaseOrder::Together=>{
                if takeover==json!({"kind":"refused","value":{"kind":"previewStale"}}) && piece_result==(ReplicationAnswer::Applied {through:original_position(2)?}) {
                    FirstCall::Piece
                } else if takeover==json!({"kind":"granted","value":{"newEpoch":2,"start":{"epoch":1,"sequence":1}}}) && piece_result==(ReplicationAnswer::Refused(PieceRefusal::StaleEpoch {newer:Epoch::new(2)?,ended_at:original_position(1)?})) {
                    FirstCall::Fence
                } else {return Err(format!("overlapped calls resolved to neither complete legal order: {takeover}: {piece_result:?}").into());}
            },
        };
        match first {
            FirstCall::Piece=>{
                if takeover!=json!({"kind":"refused","value":{"kind":"previewStale"}}) || piece_result!=(ReplicationAnswer::Applied {through:original_position(2)?}) {return Err(format!("Piece-first calls did not resolve to whole Applied/PreviewStale: {takeover}: {piece_result:?}").into());}
            },
            FirstCall::Fence=>{
                if takeover!=json!({"kind":"granted","value":{"newEpoch":2,"start":{"epoch":1,"sequence":1}}}) || piece_result!=(ReplicationAnswer::Refused(PieceRefusal::StaleEpoch {newer:Epoch::new(2)?,ended_at:original_position(1)?})) {return Err(format!("Fence-first calls did not resolve to exact grant/whole StaleEpoch cutoff: {takeover}: {piece_result:?}").into());}
                if !matches!(creation::operation_answer(&mut target.socket,OperationRequest::Lease(LeaseRequest::Renew(RenewRequest {attempt:RequestId::new(Uuid::now_v7())?,database,epoch:Epoch::new(2)?}))).await?,OperationAnswer::Lease(LeaseAnswer::Renew(RenewAnswer::Renewed {replayed:false,..}))) {return Err("Fence-first target did not own a usable actual Registry grant".into());}
            },
        }
        // Assert complete Copy state/rows, stable operation replay and no extra epoch.
        assert_copy(&mut old.socket,database,first).await?;
        if owner_post(&http,&route,&confirmation).await?!=takeover {return Err("held owner Takeover replay changed its terminal answer or allocated another epoch".into());}
        assert_copy(&mut old.socket,database,first).await?;
        println!("OWNER_FENCE_ORDERING_CASE {} passed",match first {FirstCall::Piece=>"piece_first_complete_rows_and_preview_stale",FirstCall::Fence=>"fence_first_original_prefix_and_whole_stale_piece"});
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    // Release exact owned holds even when an incorrect build fails an assertion.
    let _ = runtime_control(
        base,
        ObjectAddress::Registry,
        RuntimeControl::ResumeDispatch,
    )
    .await;
    let _ = runtime_control(base, session, RuntimeControl::ResumeDispatch).await;
    callers.abort_all();
    while callers.join_next().await.is_some() {}
    let old_closed = old
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let target_closed = target
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    old_closed?;
    target_closed?;
    Ok(())
}
async fn native_held_order(order: ReleaseOrder) -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start(TokioSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: PeerId::try_from("019f0000-0000-7000-8000-000000005240".to_owned())?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let proof = normal_held_order(server.base_url(), order).await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    proof?;
    stopped?;
    keys?;
    Ok(())
}
#[tokio::test]
async fn held_old_piece_commits_before_the_owner_fence_without_partial_rows()
-> ConformanceResult<()> {
    native_held_order(ReleaseOrder::PieceFirst).await
}
#[tokio::test]
async fn held_owner_fence_commits_before_the_old_piece_with_the_exact_cutoff()
-> ConformanceResult<()> {
    native_held_order(ReleaseOrder::FenceFirst).await
}

#[tokio::test]
async fn both_held_calls_resume_before_either_answer_and_resolve_to_one_complete_order()
-> ConformanceResult<()> {
    native_held_order(ReleaseOrder::Together).await
}
