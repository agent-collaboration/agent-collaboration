//! Correlate real Push requests and send only caller-supplied custody receipts.
use super::mail_push_inbox::PushInbox;
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_central_conformance::ConformanceResult;
use fleet_sync_protocol::{
    ClientAnswer, ClientFrame, DeliveryItemRef, FrameId, PushResult, ServerCall, ServerCallAnswer,
    ServerFrame, ServerRequest, Submitter,
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, WebSocket};
use std::time::Duration;
pub(super) async fn next_push(
    socket: &mut WebSocket,
) -> ConformanceResult<(FrameId, ServerCall<BoardDeliveryTypes>)> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket.next().await.ok_or("Push socket ended")??;
            let frame: ServerFrame<BoardConsumerProtocol> = match message {
                Message::Text(text) => serde_json::from_str(&text)?,
                Message::Binary(bytes) => serde_json::from_slice(&bytes)?,
                _ => return Err("unexpected physical Push message".into()),
            };
            match frame {
                ServerFrame::Request {
                    frame,
                    body: ServerRequest::Crate(call @ ServerCall::Push { .. }),
                } => return Ok((frame, call)),
                ServerFrame::Notice { .. }
                | ServerFrame::Request {
                    body: ServerRequest::Crate(ServerCall::PublishStatus { .. }),
                    ..
                } => {}
                _ => return Err("Push lost its normal frame correlation".into()),
            }
        }
    })
    .await?
}
pub(super) async fn pushed(
    socket: &mut WebSocket,
    frame: FrameId,
    answer: PushResult<<BoardDeliveryTypes as fleet_sync_protocol::DeliveryTypes>::Acceptance>,
) -> ConformanceResult<()> {
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Answer {
                frame,
                body: ClientAnswer::Crate(ServerCallAnswer::Pushed(answer)),
            })?
            .into(),
        ))
        .await?;
    Ok(())
}

/// Reports and retries share this physical socket. Only already committed,
/// independently identified custody may produce a replay receipt here.
pub(super) async fn recipient_operation(
    socket: &mut WebSocket,
    inbox: &PushInbox,
    submitter: Submitter,
    allowed: &[DeliveryItemRef],
    body: serde_json::Value,
) -> ConformanceResult<serde_json::Value> {
    let expected = FrameId::new(durable_steps::Uuid::now_v7())?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                frame: expected,
                body: serde_json::from_value(body)?,
            })?
            .into(),
        ))
        .await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or("recipient report socket ended")??;
            let frame: ServerFrame<BoardConsumerProtocol> = match message {
                Message::Text(text) => serde_json::from_str(&text)?,
                Message::Binary(bytes) => serde_json::from_slice(&bytes)?,
                _ => return Err("unexpected recipient report message".into()),
            };
            match frame {
                ServerFrame::Answer { frame, body } if frame == expected => {
                    return Ok(serde_json::to_value(body)?);
                }
                ServerFrame::Request {
                    frame,
                    body:
                        ServerRequest::Crate(ServerCall::Push {
                            submitter: actual,
                            item,
                            ..
                        }),
                } => {
                    let reference = DeliveryItemRef {
                        item: item.id(),
                        digest: item.digest().clone(),
                    };
                    if actual != submitter || !allowed.contains(&reference) {
                        return Err(
                            "concurrent Push changed its verified sender or known identity".into(),
                        );
                    }
                    let receipt = inbox.accept(actual, item).await?;
                    if !matches!(receipt, PushResult::Replayed { .. }) {
                        return Err(
                            "concurrent retry did not replay actual committed recipient custody"
                                .into(),
                        );
                    }
                    pushed(socket, frame, receipt).await?;
                }
                ServerFrame::Notice { .. }
                | ServerFrame::Request {
                    body: ServerRequest::Crate(ServerCall::PublishStatus { .. }),
                    ..
                } => {}
                _ => return Err("recipient report lost its exact frame correlation".into()),
            }
        }
    })
    .await?
}
