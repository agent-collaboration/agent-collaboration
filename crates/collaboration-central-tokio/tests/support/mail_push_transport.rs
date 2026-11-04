//! Correlate real Push requests and send only caller-supplied custody receipts.
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_central_conformance::ConformanceResult;
use fleet_sync_protocol::{
    ClientAnswer, ClientFrame, FrameId, PushResult, ServerCall, ServerCallAnswer, ServerFrame,
    ServerRequest,
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
