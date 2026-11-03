//! A lost committed Copy Fence reply survives an owning-server reopen.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;
#[path = "owner_takeover_recovery/grant_readback.rs"]
mod grant_readback;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/owner_registry_scenarios.rs"]
mod owner_registry_scenarios;

use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, DispatchFaultObservation, JwksFixtureServer,
    ObjectReplyFaultObservation, RuntimeControl, RuntimeControlRequest, RuntimeFaultObservation,
    enroll_admitted_peer, fixture_policy, machine_assertion, owner_assertion,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use creation_scenarios as creation;
use durable_steps::{CallId, CallTarget, Uuid};
use fleet_admission::FleetOperation;
use fleet_sync_protocol::*;
use futures_util::{SinkExt, StreamExt};
use message_board::{BoardId, Description, ResourceName, WorkspaceId};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

const OLD_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440781";
const OLD_PEER: &str = "019f0000-0000-7000-8000-000000000781";
const TARGET_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440782";
const TARGET_PEER: &str = "019f0000-0000-7000-8000-000000000782";
const OLD_CREDENTIAL: &str = "interrupted takeover old holder";
const TARGET_CREDENTIAL: &str = "interrupted takeover target";
const FIRST_KEY: &str = "019f0000-0000-7000-8000-000000000783";
const NEXT_KEY: &str = "019f0000-0000-7000-8000-000000000784";
const START: &str = "2030-03-17T17:46:40Z";

struct InterruptedTransfer {
    database: DatabaseId,
    confirmation: Value,
    fence_envelope: Value,
    old_session: SessionId,
    target_session: SessionId,
}

fn position(epoch: u64, sequence: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(epoch)?,
        CommitSequence::new(sequence),
    ))
}
fn row_values(key: &str, number: i64, label: &str) -> Vec<SqlValue> {
    vec![
        SqlValue::Text(key.into()),
        SqlValue::Integer(number),
        SqlValue::Text(label.into()),
    ]
}
fn row_piece(
    database: DatabaseId,
    epoch: u64,
    key: &str,
    label: &str,
) -> ConformanceResult<Piece<BoardAnnotation>> {
    let (through, number) = if epoch == 1 && key == FIRST_KEY {
        (position(1, 2)?, 2)
    } else if epoch == 1 {
        (position(1, 3)?, 3)
    } else {
        (position(3, 1)?, 3)
    };
    Ok(Piece::new(PieceContents {
        database,
        epoch: Epoch::new(epoch)?,
        continues_from: if key == FIRST_KEY {
            position(1, 1)?
        } else {
            position(1, 2)?
        },
        through,
        transactions: vec![ReplicatedTransaction::Rows {
            commit_id: through.sequence,
            rows: vec![RowChange::upsert(
                "takeover_rows",
                vec![SqlValue::Text(key.into())],
                row_values(key, number, label),
            )?],
        }],
        part: None,
        annotation: None,
    })?)
}
async fn control(
    base: &str,
    command: RuntimeControl,
) -> ConformanceResult<RuntimeFaultObservation> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()?
        .post(format!("{base}/admin/v1/conformance/runtime"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&RuntimeControlRequest {
            address: ObjectAddress::Registry,
            command,
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
async fn held_fence(base: &str, database: DatabaseId) -> ConformanceResult<Value> {
    let observation = control(base, RuntimeControl::AwaitHeldDispatch).await?;
    let DispatchFaultObservation::Held {
        destination,
        request,
    } = observation.dispatch
    else {
        return Err("Takeover never reached its held Copy Fence".into());
    };
    if destination != ObjectAddress::Copy(database) {
        return Err("Takeover held a different physical Copy".into());
    }
    Ok(serde_json::from_slice(&request)?)
}
fn validate_fence(
    envelope: &Value,
    database: DatabaseId,
    confirmation: &Value,
) -> ConformanceResult<()> {
    let request: FenceRequest = serde_json::from_value(
        envelope
            .get("request")
            .cloned()
            .ok_or("held Fence omitted request")?,
    )?;
    let _: CallId = serde_json::from_value(
        envelope
            .get("identity")
            .cloned()
            .ok_or("held Fence omitted CallId")?,
    )?;
    let target: fleet_sync_lease::CopyFence = serde_json::from_value(
        envelope
            .get("target")
            .cloned()
            .ok_or("held Fence omitted target")?,
    )?;
    if envelope.get("source") != Some(&json!(ObjectAddress::Registry))
        || envelope.get("destination") != Some(&json!(ObjectAddress::Copy(database)))
        || envelope.get("kind") != Some(&json!(<fleet_sync_lease::CopyFence as CallTarget>::KIND))
        || target.copy != configuration::registry_config()?.authoritative_receiver
        || request.database != database
        || confirmation.get("transferId") != Some(&json!(request.transfer_id))
        || request.new_epoch != Epoch::new(3)?
        || request.holder != PeerId::try_from(TARGET_PEER.to_owned())?
        || request.expect
            != (FenceExpectation {
                epoch: Epoch::new(1)?,
                acked: position(1, 2)?,
            })
        || request.target_schema != Some(SchemaSupport { max_version: 1 })
    {
        return Err(format!("held Fence lost its exact Registry/Copy/transfer/epoch/holder/schema/prefix: {envelope}").into());
    }
    Ok(())
}
async fn assert_copy(
    socket: &mut WebSocket,
    database: DatabaseId,
    fence: u64,
    has_new_row: bool,
) -> ConformanceResult<()> {
    let prefix = if has_new_row {
        position(3, 1)?
    } else {
        position(1, 2)?
    };
    let holder = if fence == 1 { OLD_PEER } else { TARGET_PEER };
    let status = creation::operation_answer(
        socket,
        OperationRequest::Replication(ReplicationRequest::CopyStatus { database }),
    )
    .await?;
    if !matches!(status,OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))) if status.database==database && status.role==CopyRole::Authoritative && status.fence==Epoch::new(fence)? && status.holder==Some(PeerId::try_from(holder.to_owned())?) && status.acked==prefix && status.schema.version==1)
    {
        return Err("interrupted Takeover changed Copy's exact holder/fence/schema/prefix".into());
    }
    let page = creation::operation_answer(
        socket,
        OperationRequest::Replication(ReplicationRequest::SnapshotPage {
            database,
            cursor: None,
        }),
    )
    .await?;
    let mut expected = vec![row_values(FIRST_KEY, 2, "between preview and fence")];
    if has_new_row {
        expected.push(row_values(NEXT_KEY, 3, "new holder"));
    }
    if !matches!(page,OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page))) if page.database==database && page.position==prefix && page.schema.version==1 && page.table=="takeover_rows" && page.rows==expected && page.next.is_none())
    {
        return Err(
            "interrupted Takeover lost the exact complete rows or exposed rejected tail".into(),
        );
    }
    Ok(())
}
async fn preview(
    http: &reqwest::Client,
    route: &str,
    sequence: u64,
) -> ConformanceResult<TakeoverPreview> {
    let answer =
        owner_registry_scenarios::post(http, &format!("{route}/preview"), &Value::Null).await?;
    let preview: TakeoverPreview = serde_json::from_value(
        answer
            .get("preview")
            .cloned()
            .ok_or("Takeover Preview omitted native confirmation")?,
    )?;
    if answer.get("holderInstallation") != Some(&json!(OLD_SERVICE))
        || preview.fence != Epoch::new(1)?
        || preview.holder != PeerId::try_from(OLD_PEER.to_owned())?
        || preview.acked != position(1, sequence)?
    {
        return Err("Takeover Preview did not read the real original holder/fence/prefix".into());
    }
    Ok(preview)
}
async fn arrange_and_interrupt(base: &str) -> ConformanceResult<InterruptedTransfer> {
    creation::pin_registry_clock(base, RegistryTime(START.parse()?)).await?;
    let client = ConformanceClient::new(base)?;
    let old_session = SessionId::new(Uuid::now_v7())?;
    let target_session = SessionId::new(Uuid::now_v7())?;
    let mut old = enroll_admitted_peer(
        &client,
        OLD_SERVICE,
        OLD_PEER,
        OLD_CREDENTIAL,
        &old_session.to_string(),
    )
    .await?;
    let target = enroll_admitted_peer(
        &client,
        TARGET_SERVICE,
        TARGET_PEER,
        TARGET_CREDENTIAL,
        &target_session.to_string(),
    )
    .await?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let mut callers = tokio::task::JoinSet::new();
    let proof = async {
        let database = DatabaseId::new(Uuid::now_v7())?;
        let workspace = WorkspaceId::generate(); let board = BoardId::generate();
        for operation in [
            FleetOperation::WorkspaceCreate {name: ResourceName::try_from("interrupted Takeover workspace".to_owned())?, description: Description::try_from("lost committed Fence reply".to_owned())?},
            FleetOperation::BoardCreate {board: board.clone(), name: ResourceName::try_from("interrupted Takeover board".to_owned())?, description: Description::try_from("owning server reopen".to_owned())?, board_kind: message_board::BoardKind::Area {}},
            creation::project(database, board)?,
        ] { creation::accepted(creation::metadata(&mut old.socket, &workspace, operation).await?)?; }
        creation::registered(&creation::register(&mut old.socket, database, PeerId::try_from(OLD_PEER.to_owned())?, RequestId::new(Uuid::now_v7())?).await?, PeerId::try_from(OLD_PEER.to_owned())?)?;
        let schema = Piece::<BoardAnnotation>::new(PieceContents {
            database, epoch: Epoch::new(1)?, continues_from: position(1, 0)?, through: position(1, 1)?,
            transactions: vec![ReplicatedTransaction::SchemaStep {commit_id: CommitSequence::new(1), migration_version: 1, statements: vec!["CREATE TABLE takeover_rows(record_key TEXT NOT NULL PRIMARY KEY,number INTEGER NOT NULL,label TEXT NOT NULL)".into()]}], part: None, annotation: None,
        })?;
        if !matches!(creation::operation_answer(&mut old.socket, OperationRequest::Replication(ReplicationRequest::Piece(schema))).await?,OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through==position(1,1)?) { return Err("Takeover setup did not apply schema1".into()); }
        let route = format!("{base}/admin/v1/projects/{database}/takeover");
        let stale_preview = preview(&http, &route, 1).await?;
        let rows = row_piece(database, 1, FIRST_KEY, "between preview and fence")?;
        if !matches!(creation::operation_answer(&mut old.socket, OperationRequest::Replication(ReplicationRequest::Piece(rows))).await?,OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through==position(1,2)?) { return Err("Takeover setup did not commit the between-preview-and-Fence row".into()); }
        let stale = json!({"transferId":Uuid::now_v7(),"toInstallation":TARGET_SERVICE,"reason":"stale preview preserves Copy","preview":stale_preview});
        let initial = owner_registry_scenarios::post(&http, &route, &stale).await?;
        if owner_registry_scenarios::complete(&http,&route,&stale,initial).await? != json!({"kind":"refused","value":{"kind":"previewStale"}}) { return Err("Takeover accepted a stale pre-row preview".into()); }
        assert_copy(&mut old.socket, database, 1, false).await?;
        println!("TAKEOVER_RECOVERY_STAGE stale_preview_preserves_fence1_and_literal_row passed");
        let confirmation = json!({"transferId":Uuid::now_v7(),"toInstallation":TARGET_SERVICE,"reason":"recover committed but lost Copy Fence reply","preview":preview(&http,&route,2).await?});
        control(base, RuntimeControl::HoldNextDispatch {destination: ObjectAddress::Copy(database)}).await?;
        let send_http = http.clone(); let send_route = route.clone(); let send_confirmation = confirmation.clone();
        callers.spawn(async move { owner_registry_scenarios::post(&send_http,&send_route,&send_confirmation).await });
        let fence_envelope = held_fence(base, database).await?;
        validate_fence(&fence_envelope, database, &confirmation)?;
        control(base, RuntimeControl::DropNextObjectReply {destination: ObjectAddress::Copy(database)}).await?;
        control(base, RuntimeControl::ResumeDispatch).await?;
        let dropped = control(base, RuntimeControl::AwaitDroppedObjectReply).await?;
        if !matches!(dropped.object_reply,ObjectReplyFaultObservation::Dropped {destination,request} if destination==ObjectAddress::Copy(database) && serde_json::from_slice::<Value>(&request)?==fence_envelope) { return Err("Takeover did not lose this exact successful Copy Fence reply".into()); }
        let initial = tokio::time::timeout(Duration::from_secs(8), callers.join_next()).await?.ok_or("interrupted Takeover HTTP caller disappeared")???;
        if initial != json!({"kind":"pending","value":{"transferId":confirmation.get("transferId").ok_or("confirmation omitted transfer")?}}) { return Err(format!("lost Fence reply did not leave this exact public pending transfer: {initial}").into()); }
        assert_copy(&mut old.socket, database, 3, false).await?;
        println!("TAKEOVER_RECOVERY_STAGE exact_Fence_reply_lost_after_committed_fence3_before_reopen passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(InterruptedTransfer {database, confirmation, fence_envelope, old_session, target_session})
    }.await;
    let _ = control(base, RuntimeControl::ResumeDispatch).await;
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
    let proof = proof?;
    old_closed?;
    target_closed?;
    Ok(proof)
}
async fn reconnect(
    base: &str,
    credential: &str,
    session_id: SessionId,
) -> ConformanceResult<WebSocket> {
    let mut socket = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()?
        .get(format!("{base}/v1/session"))
        .bearer_auth(credential)
        .header("Cf-Access-Jwt-Assertion", machine_assertion(credential)?)
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                session_id,
                schema_support: SchemaSupport { max_version: 1 },
            })?
            .into(),
        ))
        .await?;
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await?
        .ok_or("reopened holder ended before Welcome")??;
    let bytes = match message {
        Message::Binary(bytes) => bytes.to_vec(),
        Message::Text(text) => text.into_bytes(),
        _ => return Err("reopened holder did not receive Welcome data".into()),
    };
    if !matches!(serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)?,ServerFrame::Welcome {session_id:actual,..} if actual==session_id)
    {
        return Err("reopened holder Welcome changed its original session identity".into());
    }
    Ok(socket)
}
async fn recover_same_transfer(
    base: &str,
    interrupted: InterruptedTransfer,
) -> ConformanceResult<()> {
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let route = format!("{base}/admin/v1/projects/{}/takeover", interrupted.database);
    let replay_http = http.clone();
    let replay_route = route.clone();
    let confirmation = interrupted.confirmation.clone();
    let mut callers = tokio::task::JoinSet::new();
    callers.spawn(async move {
        owner_registry_scenarios::post(&replay_http, &replay_route, &confirmation).await
    });
    let proof = async {
        let replayed = held_fence(base, interrupted.database).await?;
        if replayed != interrupted.fence_envelope { return Err("reopened Takeover changed its exact original Fence CallId/request/target".into()); }
        println!("TAKEOVER_RECOVERY_STAGE owning_reopen_retries_exact_original_Fence_envelope passed");
        control(base, RuntimeControl::ResumeDispatch).await?;
        let initial = tokio::time::timeout(Duration::from_secs(8),callers.join_next()).await?.ok_or("reopened Takeover caller disappeared")???;
        let granted = owner_registry_scenarios::complete(&http,&route,&interrupted.confirmation,initial).await?;
        let expected = json!({"kind":"granted","value":{"newEpoch":3,"start":{"epoch":1,"sequence":2}}});
        if granted != expected || owner_registry_scenarios::post(&http,&route,&interrupted.confirmation).await? != expected { return Err(format!("same interrupted Takeover did not grant and replay exactly epoch3/start1:2: {granted}").into()); }
        println!("TAKEOVER_RECOVERY_STAGE same_attempt_grants_and_replays_exact_epoch3_start1_2 passed");
        let mut old = reconnect(base, OLD_CREDENTIAL, interrupted.old_session).await?;
        let mut target = reconnect(base, TARGET_CREDENTIAL, interrupted.target_session).await?;
        let readback = async {
            assert_copy(&mut old, interrupted.database, 3, false).await?;
            let old_piece = row_piece(interrupted.database,1,NEXT_KEY,"rejected tail")?;
            let refused = creation::operation_answer(&mut old,OperationRequest::Replication(ReplicationRequest::Piece(old_piece))).await?;
            if !matches!(refused,OperationAnswer::Replication(ReplicationAnswer::Refused(PieceRefusal::StaleEpoch {newer,ended_at})) if newer==Epoch::new(3)? && ended_at==position(1,2)?) { return Err("recovered Takeover did not reject the old holder at exact epoch3/cutoff1:2".into()); }
            assert_copy(&mut old, interrupted.database, 3, false).await?;
            let new_piece = row_piece(interrupted.database,3,NEXT_KEY,"new holder")?;
            if !matches!(creation::operation_answer(&mut target,OperationRequest::Replication(ReplicationRequest::Piece(new_piece))).await?,OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through==position(3,1)?) { return Err("recovered target did not apply its real epoch3 Piece".into()); }
            assert_copy(&mut target, interrupted.database, 3, true).await?;
            assert_copy(&mut old, interrupted.database, 3, true).await?;
            println!("TAKEOVER_RECOVERY_CASE old_holder_stale_target_applies_exact_rows_after_lost_Fence_and_owning_reopen passed");
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
        }.await;
        let old_closed=old.close(reqwest_websocket::CloseCode::Normal,None).await;
        let target_closed=target.close(reqwest_websocket::CloseCode::Normal,None).await;
        readback?;old_closed?;target_closed?;Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = control(base, RuntimeControl::ResumeDispatch).await;
    callers.abort_all();
    while callers.join_next().await.is_some() {}
    proof
}
fn settings(root: &Path, keys: &JwksFixtureServer) -> ConformanceResult<TokioSettings> {
    Ok(TokioSettings {
        root: root.into(),
        access_policy: fixture_policy(),
        jwks_url: keys.url.clone(),
        registry: configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: PeerId::try_from("019f0000-0000-7000-8000-000000005240".to_owned())?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
}
#[tokio::test]
async fn takeover_replays_committed_lost_fence_after_owning_reopen_without_second_epoch()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let keys = JwksFixtureServer::start().await?;
    let proof = async {
        let server = TokioServer::start_conformance(settings(root.path(), &keys)?).await?;
        let interrupted = arrange_and_interrupt(server.base_url()).await;
        let stopped = server.shutdown().await;
        let interrupted = interrupted?;
        stopped?;
        grant_readback::assert_history(root.path(), interrupted.database, false).await?;
        let database = interrupted.database;
        let server = TokioServer::start_with_controls(
            settings(root.path(), &keys)?,
            vec![
                RuntimeControlRequest {
                    address: ObjectAddress::Registry,
                    command: RuntimeControl::SetTime {
                        now: RegistryTime("2030-03-17T17:46:41Z".parse()?),
                    },
                },
                RuntimeControlRequest {
                    address: ObjectAddress::Registry,
                    command: RuntimeControl::HoldNextDispatch {
                        destination: ObjectAddress::Copy(interrupted.database),
                    },
                },
            ],
        )
        .await?;
        let result = recover_same_transfer(server.base_url(), interrupted).await;
        let stopped = server.shutdown().await;
        result?;
        stopped?;
        grant_readback::assert_history(root.path(), database, true).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = keys.shutdown().await;
    proof?;
    stopped?;
    Ok(())
}
