//! A lost nonempty Piece answer is read on its surviving socket before reconnect.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/creation_scenarios.rs"]
mod creation;

use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use collaboration_central_conformance::{
    AnswerFaultObservation, ConformanceClient, ConformanceResult, JwksFixtureServer,
    RuntimeControl, RuntimeControlRequest, RuntimeFaultObservation, enroll_admitted_peer,
    fixture_policy, machine_assertion, owner_assertion,
};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use durable_steps::Uuid;
use fleet_admission::FleetOperation;
use fleet_sync_protocol::*;
use futures_util::{SinkExt, StreamExt};
use message_board::{BoardId, Description, ResourceName, ServiceId, WorkspaceId};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use std::time::Duration;

const SERVICE: &str = "550e8400-e29b-41d4-a716-446655440769";
const PEER: &str = "019f0000-0000-7000-8000-000000000769";
const CREDENTIAL: &str = "nonempty lost answer holder";
const RECORD_KEY: &str = "019f0000-0000-7000-8000-000000000770";

fn position(sequence: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(1)?,
        CommitSequence::new(sequence),
    ))
}
fn original_lease(database: DatabaseId) -> ConformanceResult<LeaseStatus> {
    let start: chrono::DateTime<chrono::Utc> = "2030-03-17T17:46:40Z".parse()?;
    Ok(LeaseStatus {
        database,
        epoch: Epoch::new(1)?,
        holder: PeerId::try_from(PEER.to_owned())?,
        state: LeaseState::Held,
        expires_at: SyncTime::from_unix_millis(start.timestamp_millis() + 14_400_000),
        remaining_ms: 14_400_000,
    })
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
            address: ObjectAddress::Connection(ServiceId::try_from(SERVICE.to_owned())?),
            command,
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
async fn exact_snapshot(socket: &mut WebSocket, database: DatabaseId) -> ConformanceResult<()> {
    let status = creation::operation_answer(
        socket,
        OperationRequest::Replication(ReplicationRequest::CopyStatus { database }),
    )
    .await?;
    if !matches!(status,OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))) if status.fence==Epoch::new(1)? && status.acked==position(2)? && status.schema.version==1)
    {
        return Err("lost-row status changed its exact fence1/prefix1:2/schema1".into());
    }
    let page = creation::operation_answer(
        socket,
        OperationRequest::Replication(ReplicationRequest::SnapshotPage {
            database,
            cursor: None,
        }),
    )
    .await?;
    let expected = vec![vec![
        SqlValue::Text(RECORD_KEY.into()),
        SqlValue::Integer(1),
        SqlValue::Text("accepted once".into()),
    ]];
    if !matches!(page,OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page))) if page.database==database && page.position==position(2)? && page.schema.version==1 && page.table=="lost_rows" && page.rows==expected && page.next.is_none())
    {
        return Err(
            "lost-row snapshot did not retain exactly its literal accepted row/schema/prefix"
                .into(),
        );
    }
    Ok(())
}
async fn reconnect(
    base: &str,
    socket: &mut WebSocket,
    session_id: SessionId,
    database: DatabaseId,
) -> ConformanceResult<()> {
    // The caller performs its surviving-socket reads before this honest close.
    SinkExt::close(socket).await?;
    let mut replacement = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()?
        .get(format!("{base}/v1/session"))
        .bearer_auth(CREDENTIAL)
        .header("Cf-Access-Jwt-Assertion", machine_assertion(CREDENTIAL)?)
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    replacement
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {
                session_id,
                schema_support: SchemaSupport { max_version: 1 },
            })?
            .into(),
        ))
        .await?;
    let proof =
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let message = replacement
                    .next()
                    .await
                    .ok_or("lost-row reconnect ended before Welcome")??;
                let frame = match message {
                    Message::Binary(bytes) => {
                        serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)?
                    }
                    Message::Text(text) => serde_json::from_str(&text)?,
                    other => {
                        return Err(format!(
                            "lost-row reconnect expected Welcome data, received {other:?}"
                        )
                        .into());
                    }
                };
                match frame {
                    ServerFrame::Notice { .. } => {}
                    ServerFrame::Welcome {
                        session_id: actual,
                        databases,
                    } if actual == session_id
                        && databases
                            == vec![DatabaseStatus::Ready {
                                database,
                                acked: position(2)?,
                                lease: original_lease(database)?,
                            }] =>
                    {
                        return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                    }
                    _ => return Err(
                        "lost-row reconnect Welcome changed the exact original session/ack/lease"
                            .into(),
                    ),
                }
            }
        })
        .await?;
    if let Err(error) = proof {
        let _ = replacement
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        return Err(error);
    }
    *socket = replacement;
    Ok(())
}
async fn replay(socket: &mut WebSocket, piece: &Piece<BoardAnnotation>) -> ConformanceResult<()> {
    if !matches!(creation::operation_answer(socket,OperationRequest::Replication(ReplicationRequest::Piece(piece.clone()))).await?,OperationAnswer::Replication(ReplicationAnswer::AlreadyApplied {through}) if through==position(2)?)
    {
        return Err("lost-row same Piece did not replay exact AlreadyApplied1:2".into());
    }
    Ok(())
}
async fn run_normal_case(base: &str) -> ConformanceResult<()> {
    creation::pin_registry_clock(base, RegistryTime("2030-03-17T17:46:40Z".parse()?)).await?;
    let session_id = SessionId::new(Uuid::now_v7())?;
    let mut writer = enroll_admitted_peer(
        &ConformanceClient::new(base)?,
        SERVICE,
        PEER,
        CREDENTIAL,
        &session_id.to_string(),
    )
    .await?;
    let proof=async {
        let database=DatabaseId::new(Uuid::now_v7())?;let workspace=WorkspaceId::generate();let board=BoardId::generate();
        for operation in [
            FleetOperation::WorkspaceCreate {name:ResourceName::try_from("lost row workspace".to_owned())?,description:Description::try_from("literal lost Piece and surviving socket".to_owned())?},
            FleetOperation::BoardCreate {board:board.clone(),name:ResourceName::try_from("lost row board".to_owned())?,description:Description::try_from("normal admitted creation".to_owned())?,board_kind:message_board::BoardKind::Area {}},
            creation::project(database,board)?,
        ] {creation::accepted(creation::metadata(&mut writer.socket,&workspace,operation).await?)?;}
        creation::registered(&creation::register(&mut writer.socket,database,PeerId::try_from(PEER.to_owned())?,RequestId::new(Uuid::now_v7())?).await?,PeerId::try_from(PEER.to_owned())?)?;
        let schema=Piece::<BoardAnnotation>::new(PieceContents {database,epoch:Epoch::new(1)?,continues_from:position(0)?,through:position(1)?,transactions:vec![ReplicatedTransaction::SchemaStep {commit_id:CommitSequence::new(1),migration_version:1,statements:vec!["CREATE TABLE lost_rows(record_key TEXT NOT NULL PRIMARY KEY,number INTEGER NOT NULL,label TEXT NOT NULL)".into()]}],part:None,annotation:None})?;
        if !matches!(creation::operation_answer(&mut writer.socket,OperationRequest::Replication(ReplicationRequest::Piece(schema))).await?,OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through==position(1)?) {return Err("lost-row setup did not commit its normal schema1 prefix".into());}
        let piece=Piece::<BoardAnnotation>::new(PieceContents {database,epoch:Epoch::new(1)?,continues_from:position(1)?,through:position(2)?,transactions:vec![ReplicatedTransaction::Rows {commit_id:CommitSequence::new(2),rows:vec![RowChange::upsert("lost_rows",vec![SqlValue::Text(RECORD_KEY.into())],vec![SqlValue::Text(RECORD_KEY.into()),SqlValue::Integer(1),SqlValue::Text("accepted once".into())])?]}],part:None,annotation:None})?;
        let frame=FrameId::new(Uuid::now_v7())?;
        control(base,RuntimeControl::DropNextAnswer {frame}).await?;
        writer.socket.send(Message::Binary(serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {frame,body:OperationRequest::Replication(ReplicationRequest::Piece(piece.clone()))})?.into())).await?;
        if !matches!(control(base,RuntimeControl::AwaitDroppedAnswer {frame}).await?.answer,AnswerFaultObservation::Dropped {frame:actual} if actual==frame) {return Err("nonempty Piece did not lose its exact outgoing frame answer".into());}
        println!("SESSION_LOST_ROW_STAGE exact_nonempty_Piece_answer_dropped passed");
        // Transport evidence: no close, new Hello, reopen or eviction occurred.
        exact_snapshot(&mut writer.socket,database).await?;
        println!("SESSION_LOST_ROW_STAGE original_surviving_socket_read_before_any_reconnect passed");
        reconnect(base,&mut writer.socket,session_id,database).await?;
        println!("SESSION_LOST_ROW_STAGE first_exact_Welcome_reconciles_nonempty_prefix passed");
        replay(&mut writer.socket,&piece).await?;
        exact_snapshot(&mut writer.socket,database).await?;
        println!("SESSION_LOST_ROW_STAGE same_replacement_socket_exact_read_after_replay passed");
        reconnect(base,&mut writer.socket,session_id,database).await?;
        replay(&mut writer.socket,&piece).await?;
        exact_snapshot(&mut writer.socket,database).await?;
        println!("SESSION_LOST_ROW_CASE literal_nonempty_lost_answer_same_socket_reads_and_two_reconnects passed");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = writer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}

#[tokio::test]
async fn lost_nonempty_piece_is_read_without_new_hello_and_reconciles_across_two_reconnects()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let keys = JwksFixtureServer::start().await?;
    let server = TokioServer::start(TokioSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: keys.url.clone(),
        registry: configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: PeerId::try_from("019f0000-0000-7000-8000-000000005240".to_owned())?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let proof = run_normal_case(server.base_url()).await;
    let stopped = server.shutdown().await;
    let keys = keys.shutdown().await;
    proof?;
    stopped?;
    keys?;
    Ok(())
}
