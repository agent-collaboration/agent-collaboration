//! Retained board operations must answer on the authenticated machine session.
#![cfg(feature = "conformance")]

#[path = "native_http/support.rs"]
mod configuration;

use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardPayload, BoardReadPage,
    BoardReadScope, ForwardedPost, ForwardedPostKind,
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, enroll_admitted_peer, fixture_policy,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_sync_protocol::{ClientFrame, FrameId, OperationAnswer, OperationRequest, ServerFrame};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::Message;
use std::time::Duration;

enum BoardRouteCase {
    Notices,
    Directory,
    ForwardedPost,
}

#[tokio::test]
async fn directory_read_answers_the_newly_admitted_installation_instead_of_closing()
-> ConformanceResult<()> {
    run_case(BoardRouteCase::Directory).await
}

#[tokio::test]
async fn notices_read_answers_the_newly_admitted_installation_instead_of_closing()
-> ConformanceResult<()> {
    run_case(BoardRouteCase::Notices).await
}

#[tokio::test]
async fn valid_forwarded_post_gets_a_correlated_typed_outcome_instead_of_closing()
-> ConformanceResult<()> {
    run_case(BoardRouteCase::ForwardedPost).await
}

async fn run_case(case: BoardRouteCase) -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
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
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let proof = async {
        let client = ConformanceClient::new(server.base_url())?;
        let mut socket = enroll_admitted_peer(
            &client,
            &durable_steps::Uuid::now_v7().to_string(),
            &durable_steps::Uuid::now_v7().to_string(),
            "board-route-proof",
            &durable_steps::Uuid::now_v7().to_string(),
        )
        .await?;
        let proof = async {
            let body = match case {
                BoardRouteCase::Directory => BoardConsumerRequest::Read {
                    scope: BoardReadScope::Directory {
                        query: BoardPayload::encode(&fleet_admission::DirectoryQuery::default())?,
                    },
                    cursor: None,
                },
                BoardRouteCase::Notices => BoardConsumerRequest::Read {
                    scope: BoardReadScope::Notices,
                    cursor: None,
                },
                BoardRouteCase::ForwardedPost => BoardConsumerRequest::ForwardedPost {
                    project: message_board::ProjectId::try_from(
                        durable_steps::Uuid::now_v7().to_string(),
                    )?,
                    post: ForwardedPost {
                        kind: ForwardedPostKind::ThreadPost,
                        body: BoardPayload::encode(
                            &serde_json::json!({"body":"forward route proof"}),
                        )?,
                    },
                },
            };
            let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
            socket
                .socket
                .send(Message::Binary(
                    serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                        frame,
                        body: OperationRequest::Consumer(body),
                    })?
                    .into(),
                ))
                .await?;
            let received = tokio::time::timeout(Duration::from_secs(5), socket.socket.next())
                .await?
                .ok_or("board operation closed without its correlated answer")??;
            let bytes = match received {
                Message::Binary(bytes) => bytes.to_vec(),
                Message::Text(text) => text.into_bytes(),
                other => {
                    return Err(format!(
                        "valid board operation closed instead of answering: {other:?}"
                    )
                    .into());
                }
            };
            let ServerFrame::Answer {
                frame: actual,
                body: OperationAnswer::Consumer(answer),
            } = serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)?
            else {
                return Err("valid board operation did not receive a typed consumer answer".into());
            };
            if actual != frame {
                return Err("board operation lost its frame correlation".into());
            }
            match (case, answer) {
                (
                    BoardRouteCase::Directory,
                    BoardConsumerAnswer::Read(BoardReadPage::Directory { entries, .. }),
                ) if entries.is_empty() => Ok(()),
                (
                    BoardRouteCase::Notices,
                    BoardConsumerAnswer::Read(BoardReadPage::Notices(
                        board_sync_protocol::NoticeRead::Page { notices, .. },
                    )),
                ) if notices.is_empty() => Ok(()),
                (BoardRouteCase::ForwardedPost, BoardConsumerAnswer::ForwardedPost(_)) => Ok(()),
                _ => Err("board operation returned a different outcome family".into()),
            }
        }
        .await;
        let _ = socket
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        proof
    }
    .await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}
