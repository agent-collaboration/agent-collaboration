//! The normal socket binds writer authority before the real Copy apply.
#![cfg(feature = "conformance")]
#![allow(clippy::panic_in_result_fn)]
#[path = "copy_receiver_session/bootstrap_refusals.rs"]
mod bootstrap_refusals;
#[path = "native_http/support.rs"]
mod configuration;
#[path = "support/copy_wait_scenarios.rs"]
#[allow(
    dead_code,
    reason = "Reuse the Copy creation caller without repeating its wait scenarios."
)]
mod copy_wait_scenarios;
use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, enroll_admitted_peer, fixture_policy,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_sync_protocol::*;
use futures_util::{SinkExt, StreamExt};
use message_board::RecordTime;
use reqwest_websocket::{Message, WebSocket};
use serde_json::json;
use sqlx_core::{connection::ConnectOptions, query::query, row::Row};

#[tokio::test]
async fn a_normal_session_applies_annotations_and_another_peer_cannot_replay_its_receipt()
-> ConformanceResult<()> {
    let jwks = JwksFixtureServer::start().await?;
    let root = tempfile::tempdir()?;
    let server = TokioServer::start(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let database = DatabaseId::try_from("019f0000-0000-7000-8000-000000000b83".to_owned())?;
    let proof=async {
        let client=ConformanceClient::new(server.base_url())?;
        let mut holder=enroll_admitted_peer(&client,"550e8400-e29b-41d4-a716-4466554400b8","019f0000-0000-7000-8000-00000000100a","copy-writer","019f0000-0000-7000-8000-000000000b81").await?;
        let mut other=enroll_admitted_peer(&client,"550e8400-e29b-41d4-a716-4466554400b9","019f0000-0000-7000-8000-00000000100b","copy-other","019f0000-0000-7000-8000-000000000b82").await?;
        let result=async {
            let copies=copy_wait_scenarios::CopyClient::new(server.base_url())?;
            let creation=copies.invoke(json!({"kind":"create","value":{"database":database,"epoch":1,"holder":holder.peer,"origin":{"kind":"empty"}}})).await?;
            copies.completed(creation,copy_wait_scenarios::expected("created",CreateCopyAnswer::CopyCreated)).await?;
            let position=|sequence| -> ConformanceResult<ReplicationPosition>{Ok(ReplicationPosition::new(Epoch::new(1)?,CommitSequence::new(sequence)))};
            let piece=Piece::new(PieceContents{database,epoch:Epoch::new(1)?,continues_from:position(0)?,through:position(1)?,
                transactions:vec![ReplicatedTransaction::SchemaStep{commit_id:CommitSequence::new(1),migration_version:1,statements:vec!["CREATE TABLE board_rows(id TEXT NOT NULL PRIMARY KEY,value TEXT NOT NULL)".into()]}],part:None,
                annotation:Some(BoardAnnotation::ProjectSummary{status:None,latest_record_at:RecordTime::LegacyUnknown})})?;
            assert_eq!(request(&mut holder.socket,ReplicationRequest::Piece(piece.clone())).await?,ReplicationAnswer::Applied{through:position(1)?});
            assert_eq!(request(&mut holder.socket,ReplicationRequest::Piece(piece.clone())).await?,ReplicationAnswer::AlreadyApplied{through:position(1)?});
            assert_eq!(request(&mut other.socket,ReplicationRequest::Piece(piece)).await?,ReplicationAnswer::Refused(PieceRefusal::NotWriter{current:Some(Epoch::new(1)?)}));
            assert!(matches!(request(&mut other.socket,ReplicationRequest::CopyStatus{database}).await?,ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status)) if status.database==database && status.acked==position(1)?));
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
        }.await;
        let closed_holder=holder.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        let closed_other=other.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        result?;closed_holder?;closed_other?;Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    proof?;
    // Read the stopped physical owner, independently of the receiver's return value.
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.path().join(format!("copy-{database}.db")))
        .read_only(true)
        .connect()
        .await?;
    let row = query::<sqlx_turso::Turso>("SELECT record FROM _copy_summary")
        .fetch_one(&mut connection)
        .await?;
    let summary: serde_json::Value = serde_json::from_str(&row.try_get::<String, _>("record")?)?;
    assert_eq!(
        summary.get("summary"),
        Some(
            &json!({"kind":"projectSummary","value":{"status":null,"latestRecordAt":{"kind":"legacyUnknown"}}})
        )
    );
    assert_eq!(
        summary.get("position"),
        Some(&serde_json::to_value(ReplicationPosition::new(
            Epoch::new(1)?,
            CommitSequence::new(1)
        ))?)
    );
    sqlx_core::connection::Connection::close(connection).await?;
    Ok(())
}
async fn request(
    socket: &mut WebSocket,
    body: ReplicationRequest<BoardAnnotation>,
) -> ConformanceResult<ReplicationAnswer> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                frame,
                body: OperationRequest::Replication(body),
            })?
            .into(),
        ))
        .await?;
    let response = tokio::time::timeout(std::time::Duration::from_secs(8), socket.next())
        .await?
        .ok_or("session ended without Copy answer")??;
    let bytes = match response {
        Message::Text(text) => text.into_bytes(),
        Message::Binary(bytes) => bytes.to_vec(),
        other => return Err(format!("unexpected Copy response: {other:?}").into()),
    };
    match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
        ServerFrame::Answer {
            frame: actual,
            body: OperationAnswer::Replication(answer),
        } if actual == frame => Ok(answer),
        _ => Err("Copy response lost frame correlation".into()),
    }
}
