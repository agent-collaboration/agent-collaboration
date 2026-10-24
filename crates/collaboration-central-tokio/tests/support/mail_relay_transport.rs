//! Socket correlation and original CallId transport shared by real runtime cases.
use board_sync_protocol::BoardConsumerProtocol;
use collaboration_central_conformance::ConformanceResult;
use durable_steps::Uuid;
use fleet_sync_protocol::{FrameId, ServerCall, ServerFrame, ServerRequest};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, WebSocket};
use serde_json::{Value, json};
use std::time::Duration;
pub(super) async fn operation(socket: &mut WebSocket, body: Value) -> ConformanceResult<Value> {
    let frame = FrameId::new(Uuid::now_v7())?;
    socket
        .send(Message::Text(
            json!({"kind":"request","value":{"frame":frame,"body":body}}).to_string(),
        ))
        .await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket.next().await.ok_or("mail relay socket ended")??;
            let answer = match message {
                Message::Text(text) => serde_json::from_str::<Value>(&text)?,
                Message::Binary(bytes) => serde_json::from_slice::<Value>(&bytes)?,
                other => return Err(format!("mail relay received {other:?}").into()),
            };
            match serde_json::from_value::<ServerFrame<BoardConsumerProtocol>>(answer.clone())? {
                ServerFrame::Answer {
                    frame: actual,
                    body,
                } if actual == frame => {
                    return Ok(serde_json::to_value(body)?);
                }
                // Central's parallel status publication is a real request, not
                // this frame's answer. Router application/ack remains G3's proof.
                ServerFrame::Request {
                    body: ServerRequest::Crate(ServerCall::PublishStatus { .. }),
                    ..
                } => {}
                // Mailbox custody also journals a recipient-only wake hint.
                // The real Router's SessionPump routes notices independently
                // from pending answers; this helper preserves that interleaving.
                ServerFrame::Notice {
                    body: fleet_sync_protocol::Notice::Consumer(notice),
                    ..
                } if matches!(
                    notice.kind,
                    board_sync_protocol::BoardNotice::MailWaiting { .. }
                ) => {}
                _ => return Err(format!("mail relay lost frame correlation: {answer}").into()),
            }
        }
    })
    .await?
}
pub(super) fn relay_request(kind: &str, value: Value) -> Value {
    #[derive(serde::Serialize)]
    struct CallFields {
        call: Value,
        #[serde(flatten)]
        fields: Value,
    }
    let value = CallFields {
        call: json!({"operation":Uuid::now_v7(),"step":"normal_relay"}),
        fields: value,
    };
    json!({"kind":"delivery","value":{"kind":kind,"value":value}})
}
