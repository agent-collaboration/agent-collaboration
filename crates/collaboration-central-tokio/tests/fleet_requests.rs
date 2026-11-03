//! Normal Fleet frames use the canonical owner, provenance and replay ledger.
#![cfg(feature = "conformance")]

use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardPayload,
    FleetRequestEnvelope,
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, enroll_admitted_peer, fixture_policy,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use durable_steps::Uuid;
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use fleet_sync_protocol::{ClientFrame, FrameId, OperationAnswer, OperationRequest, ServerFrame};
use futures_util::{SinkExt, StreamExt};
use message_board::{ChangeOutcome, Record, RequestDecision};
use reqwest_websocket::{Message, WebSocket};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};
use std::time::Duration;

#[path = "native_http/support.rs"]
mod configuration;
#[path = "fleet_requests/feed_reads.rs"]
mod feed_reads;
#[path = "fleet_requests/snapshot_reads.rs"]
mod snapshot_reads;
const SERVICE: &str = "550e8400-e29b-41d4-a716-4466554400f4";
const WORKSPACE: &str = "019f0000-0000-7000-8000-000000000f41";

#[tokio::test]
async fn archiving_nine_empty_boards_returns_all_ten_exact_effect_identities()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start_conformance(settings(root.path(), &jwks.url)?).await?;
    let proof = async {
        let client = ConformanceClient::new(server.base_url())?;
        let mut peer = enroll_admitted_peer(
            &client,
            &Uuid::now_v7().to_string(),
            &Uuid::now_v7().to_string(),
            "archive-ten-effects",
            &Uuid::now_v7().to_string(),
        )
        .await?;
        let proof = async {
            let workspace = message_board::WorkspaceId::try_from(Uuid::now_v7().to_string())?;
            let mut expected: Vec<message_board::RecordRef> = vec![workspace.clone().into()];
            let offered = |operation| {
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(FleetRequest {
                    request_id: message_board::RequestId::try_from(Uuid::now_v7().to_string())?,
                    workspace: workspace.clone(),
                    actor: message_board::RecordedActor::Human {
                        human_id: message_board::HumanId::try_from("archive fixture".to_owned())?,
                    },
                    acting_for: None,
                    protocol_version: message_board::ProtocolVersion::CURRENT,
                    operation,
                })
            };
            let create = offered(FleetOperation::WorkspaceCreate {
                name: message_board::ResourceName::try_from("many boards".to_owned())?,
                description: message_board::Description::try_from("receipt budget".to_owned())?,
            })?;
            if !matches!(
                decided(
                    request(&mut peer.socket, &create, create.request_id.clone()).await?,
                    &create
                )?,
                FleetRequestResult::Decided(RequestDecision::Accepted { .. })
            ) {
                return Err("archive fixture workspace was not accepted".into());
            }
            for index in 0..9 {
                let board = message_board::BoardId::try_from(Uuid::now_v7().to_string())?;
                expected.push(board.clone().into());
                let create = offered(FleetOperation::BoardCreate {
                    board,
                    name: message_board::ResourceName::try_from(format!("board {index}"))?,
                    description: message_board::Description::try_from("empty board".to_owned())?,
                    board_kind: message_board::BoardKind::Area {},
                })?;
                if !matches!(
                    decided(
                        request(&mut peer.socket, &create, create.request_id.clone()).await?,
                        &create
                    )?,
                    FleetRequestResult::Decided(RequestDecision::Accepted { .. })
                ) {
                    return Err("archive fixture board was not accepted".into());
                }
            }
            let archive = offered(FleetOperation::WorkspaceArchive {})?;
            let FleetRequestResult::Decided(RequestDecision::Accepted { effects, .. }) = decided(
                request(&mut peer.socket, &archive, archive.request_id.clone()).await?,
                &archive,
            )?
            else {
                return Err("nine boards and their workspace were not archived".into());
            };
            if effects.total_count() != 10
                || effects.entries().len() != 10
                || effects.next().is_some()
            {
                return Err(
                    "workspace archive truncated or paged the ten original receipt identities"
                        .into(),
                );
            }
            let mut actual = effects
                .entries()
                .iter()
                .map(|(reference, _)| reference.clone())
                .collect::<Vec<_>>();
            expected.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
            actual.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
            if actual != expected {
                return Err("workspace archive changed its ten exact receipt identities".into());
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let closed = peer
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        proof?;
        closed?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}

fn settings(root: &std::path::Path, jwks: &str) -> ConformanceResult<TokioSettings> {
    Ok(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.into(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
}

async fn request(
    socket: &mut WebSocket,
    request: &FleetRequest,
    envelope_id: message_board::RequestId,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    operation(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Fleet(FleetRequestEnvelope {
            request_id: envelope_id,
            operation: BoardPayload::encode(request)?,
        })),
    )
    .await
}
async fn operation(
    socket: &mut WebSocket,
    body: OperationRequest<BoardConsumerProtocol>,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    let frame = FrameId::new(Uuid::now_v7())?;
    socket
        .send(Message::Text(serde_json::to_string(&ClientFrame::<
            BoardConsumerProtocol,
        >::Request {
            frame,
            body,
        })?))
        .await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let message = tokio::time::timeout_at(deadline, socket.next())
            .await?
            .ok_or("Fleet request closed its admitted session before answering")??;
        let bytes = match message {
            Message::Text(text) => text.into_bytes(),
            Message::Binary(bytes) => bytes.to_vec(),
            other => return Err(format!("Fleet request returned {other:?}").into()),
        };
        match serde_json::from_slice(&bytes)? {
            ServerFrame::<BoardConsumerProtocol>::Answer {
                frame: actual,
                body,
            } if actual == frame => return Ok(body),
            // Feed hints can arrive independently between this read's request
            // and answer. The separate notice scenarios assert their contents.
            ServerFrame::Notice {
                body:
                    fleet_sync_protocol::Notice::Consumer(board_sync_protocol::PositionedBoardNotice {
                        kind: board_sync_protocol::BoardNotice::FleetFeedAdvanced { .. },
                        ..
                    }),
                ..
            } => {}
            _ => return Err("Fleet request lost its frame correlation".into()),
        }
    }
}
fn decided(
    answer: OperationAnswer<BoardConsumerProtocol>,
    request: &FleetRequest,
) -> ConformanceResult<FleetRequestResult> {
    match answer {
        OperationAnswer::Consumer(BoardConsumerAnswer::Fleet {
            request_id,
            decision,
        }) if request_id == request.request_id => Ok(decision.decode()?),
        _ => Err("Fleet request returned a different operation or request identity".into()),
    }
}

#[tokio::test]
async fn normal_fleet_request_keeps_its_decision_and_admitted_origin_after_reopen()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start_conformance(settings(root.path(), &jwks.url)?).await?;
    let proof = async {
        let client = ConformanceClient::new(server.base_url())?;
        let mut peer = enroll_admitted_peer(&client,SERVICE,"019f0000-0000-7000-8000-000000001010","fleet-frame-origin","019f0000-0000-7000-8000-000000000f42").await?;
        let proof = async {
            let offered = FleetRequest {
                request_id: message_board::RequestId::try_from(Uuid::now_v7().to_string())?,
                workspace: message_board::WorkspaceId::try_from(WORKSPACE.to_owned())?,
                actor: message_board::RecordedActor::Human { human_id: message_board::HumanId::try_from("fleet frame owner".to_owned())? },
                acting_for: None, protocol_version: message_board::ProtocolVersion::CURRENT,
                operation: FleetOperation::WorkspaceCreate { name: message_board::ResourceName::try_from("Socket fleet".to_owned())?, description: message_board::Description::try_from("Created through the admitted session".to_owned())? },
            };
            let first = decided(request(&mut peer.socket,&offered,offered.request_id.clone()).await?,&offered)?;
            if !matches!(&first,FleetRequestResult::Decided(RequestDecision::Accepted { change: ChangeOutcome::Changed, .. })) {
                return Err(format!("normal Fleet request was not accepted: {first:?}").into());
            }
            if decided(request(&mut peer.socket,&offered,offered.request_id.clone()).await?,&offered)? != first {
                return Err("Fleet replay changed its canonical decision".into());
            }
            let mut conflicting = offered.clone();
            conflicting.operation = FleetOperation::WorkspaceCreate { name: message_board::ResourceName::try_from("Conflicting body".to_owned())?, description: message_board::Description::try_from("must not replace the first decision".to_owned())? };
            if decided(request(&mut peer.socket,&conflicting,conflicting.request_id.clone()).await?,&conflicting)? != FleetRequestResult::ConflictingRequest {
                return Err("Fleet request identity was reused for a different body".into());
            }
            let mismatch = request(&mut peer.socket,&offered,message_board::RequestId::try_from(Uuid::now_v7().to_string())?).await?;
            if !matches!(mismatch,OperationAnswer::InvalidShape { field } if field == "fleetRequest") {
                return Err("Fleet envelope accepted a different inner request identity".into());
            }
            let mut forged = offered.clone();
            forged.request_id = message_board::RequestId::try_from(Uuid::now_v7().to_string())?;
            forged.actor = message_board::RecordedActor::Router {
                component: message_board::RouterComponent::SubscriptionDelivery,
                installation: message_board::ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400f5".to_owned())?,
            };
            if !matches!(request(&mut peer.socket,&forged,forged.request_id.clone()).await?, OperationAnswer::InvalidShape { field } if field == "fleetRequest") {
                return Err("Fleet accepted a Router actor from a different installation".into());
            }
            if decided(request(&mut peer.socket,&offered,offered.request_id.clone()).await?,&offered)? != first {
                return Err("invalid Fleet envelope made its session unusable".into());
            }
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>((offered, first))
        }.await;
        let _ = peer.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        proof
    }.await;
    server.shutdown().await?;
    let proof = async {
        let (offered, first) = proof?;
        let reopened = TokioServer::start_conformance(settings(root.path(), &jwks.url)?).await?;
        let replay = async {
            let client = ConformanceClient::new(reopened.base_url())?;
            let mut peer = enroll_admitted_peer(
                &client,
                SERVICE,
                "019f0000-0000-7000-8000-000000001010",
                "fleet-frame-origin",
                "019f0000-0000-7000-8000-000000000f43",
            )
            .await?;
            let replay = async {
                if decided(
                    request(&mut peer.socket, &offered, offered.request_id.clone()).await?,
                    &offered,
                )? != first
                {
                    return Err("reopened Fleet owner changed its saved decision".into());
                }
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
            }
            .await;
            let _ = peer
                .socket
                .close(reqwest_websocket::CloseCode::Normal, None)
                .await;
            replay
        }
        .await;
        reopened.shutdown().await?;
        replay
    }
    .await;
    jwks.shutdown().await?;
    proof?;

    // The owning native server is stopped before the actual Fleet file is read.
    let mut reader = sqlx_turso::TursoConnectOptions::new()
        .filename(root.path().join("fleet.db"))
        .read_only(true)
        .connect()
        .await?;
    let proof = async {
        let row = query::<sqlx_turso::Turso>("SELECT payload, origin_installation, origin_machine, (SELECT count(*) FROM fleet_request_decisions) AS decisions, (SELECT count(*) FROM fleet_decision_receipts) AS receipts, (SELECT count(*) FROM fleet_feed) AS feed FROM workspaces WHERE workspace_id = ?")
            .bind(WORKSPACE).fetch_one(&mut reader).await?;
        let payload: String = row.try_get("payload")?;
        let record: Record = serde_json::from_str(&payload)?;
        if !matches!(record,Record::Workspace(value) if value.workspace_id.as_str() == WORKSPACE && value.name.as_str() == "Socket fleet")
            || row.try_get::<String,_>("origin_installation")? != SERVICE
            || row.try_get::<String,_>("origin_machine")? != "fleet-frame-origin"
            || row.try_get::<i64,_>("decisions")? != 1 || row.try_get::<i64,_>("receipts")? != 1
            || row.try_get::<i64,_>("feed")? != 2 {
            return Err("Fleet reopen lost the canonical record, admitted provenance or exact replay counts".into());
        }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    reader.close().await?;
    proof
}
