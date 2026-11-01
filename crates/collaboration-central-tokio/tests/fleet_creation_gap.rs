//! Directory acceptance followed by registration creates the first usable epoch.
#![cfg(feature = "conformance")]

#[path = "native_http/support.rs"]
mod configuration;
use board_sync_protocol::{
    BoardAnnotation, BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest,
    BoardPayload, FleetRequestEnvelope,
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, enroll_admitted_peer, fixture_policy,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use fleet_sync_protocol::{
    ClientFrame, CommitSequence, DatabaseId, Epoch, FrameId, LeaseAnswer, LeaseRequest,
    OperationAnswer, OperationRequest, Origin, PeerId, Piece, PieceContents, Registration,
    RegistrationAnswer, ReplicatedTransaction, ReplicationAnswer, ReplicationPosition,
    ReplicationRequest, RequestId, ServerFrame,
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, WebSocket};
use std::time::Duration;

#[tokio::test]
async fn accepted_directory_then_registration_makes_the_first_piece_usable_on_the_same_session()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start_conformance(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let proof = async {
        let client = ConformanceClient::new(server.base_url())?;
        let mut peer = enroll_admitted_peer(&client, &durable_steps::Uuid::now_v7().to_string(), &durable_steps::Uuid::now_v7().to_string(), "ordinary-first-epoch", &durable_steps::Uuid::now_v7().to_string()).await?;
        let proof = async {
            let workspace = message_board::WorkspaceId::try_from(durable_steps::Uuid::now_v7().to_string())?;
            let board = message_board::BoardId::try_from(durable_steps::Uuid::now_v7().to_string())?;
            let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
            for operation in [
                FleetOperation::WorkspaceCreate { name: message_board::ResourceName::try_from("ordinary workspace".to_owned())?, description: message_board::Description::try_from("ordinary creation".to_owned())? },
                FleetOperation::BoardCreate { board: board.clone(), name: message_board::ResourceName::try_from("ordinary board".to_owned())?, description: message_board::Description::try_from("ordinary creation".to_owned())?, board_kind: message_board::BoardKind::Area {} },
                FleetOperation::ProjectDirectoryCreate { project: message_board::ProjectId::try_from(database.to_string())?, board, title: message_board::ResourceName::try_from("ordinary project".to_owned())? },
            ] {
                let request = FleetRequest {
                    request_id: message_board::RequestId::try_from(durable_steps::Uuid::now_v7().to_string())?, workspace: workspace.clone(),
                    actor: message_board::RecordedActor::Human { human_id: message_board::HumanId::try_from("owner".to_owned())? }, acting_for: None,
                    protocol_version: message_board::ProtocolVersion::CURRENT, operation,
                };
                let OperationAnswer::Consumer(BoardConsumerAnswer::Fleet { request_id, decision }) = answer(&mut peer.socket,
                    OperationRequest::Consumer(BoardConsumerRequest::Fleet(FleetRequestEnvelope { request_id: request.request_id.clone(), operation: BoardPayload::encode(&request)? })),
                ).await? else { return Err("ordinary metadata did not return its Fleet decision".into()); };
                if request_id != request.request_id || !matches!(decision.decode::<FleetRequestResult>()?, FleetRequestResult::Decided(message_board::RequestDecision::Accepted { .. })) {
                    return Err("ordinary workspace/board/project was not accepted".into());
                }
            }
            // Fleet owns only its committed decision. The session must make
            // the second, explicit call to Registry before sending any piece.
            let writer = PeerId::new(peer.peer)?;
            let registered = answer(&mut peer.socket, OperationRequest::Lease(LeaseRequest::Register(Registration {
                attempt: RequestId::new(durable_steps::Uuid::now_v7())?,
                database, writer, origin: Origin::Empty,
            }))).await?;
            if !matches!(registered, OperationAnswer::Lease(LeaseAnswer::Register(
                RegistrationAnswer::Registered {ref lease,replayed:false}
            )) if lease.epoch == Epoch::new(1)? && lease.holder == writer) {
                return Err("accepted directory creation did not admit its separate normal registration".into());
            }
            let epoch = Epoch::new(1)?;
            let through = ReplicationPosition::new(epoch, CommitSequence::new(1));
            let piece = Piece::<BoardAnnotation>::new(PieceContents {
                database, epoch, continues_from: ReplicationPosition::new(epoch, CommitSequence::new(0)), through,
                transactions: vec![ReplicatedTransaction::SchemaStep { commit_id: CommitSequence::new(1), migration_version: 1, statements: vec!["CREATE TABLE records(id TEXT NOT NULL PRIMARY KEY,label TEXT NOT NULL)".into()] }], part: None, annotation: None,
            })?;
            let applied = answer(&mut peer.socket, OperationRequest::Replication(ReplicationRequest::Piece(piece))).await?;
            if !matches!(&applied, OperationAnswer::Replication(ReplicationAnswer::Applied { through: actual }) if *actual == through) {
                return Err(format!("accepted directory creation did not install its first usable epoch: {}", serde_json::to_string(&applied)?).into());
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }.await;
        let _ = peer.socket.close(reqwest_websocket::CloseCode::Normal, None).await;
        proof
    }.await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}

async fn answer(
    socket: &mut WebSocket,
    body: OperationRequest<BoardConsumerProtocol>,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request { frame, body })?
                .into(),
        ))
        .await?;
    // Registry grants also publish numbered notices on this same socket. Keep
    // the answer correlation and one overall deadline across that valid prefix.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let received = socket
                .next()
                .await
                .ok_or("ordinary session closed before its answer")??;
            let bytes = match received {
                Message::Binary(bytes) => bytes.to_vec(),
                Message::Text(text) => text.into_bytes(),
                other => return Err(format!("ordinary session received {other:?}").into()),
            };
            match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
                ServerFrame::Answer {
                    frame: actual,
                    body,
                } if actual == frame => return Ok(body),
                ServerFrame::Notice { .. } => {}
                other => {
                    return Err(format!(
                        "ordinary operation expected answer {frame:?}, received {}",
                        serde_json::to_string(&other)?
                    )
                    .into());
                }
            }
        }
    })
    .await?
}
