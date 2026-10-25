//! Shared test transport uses normal frames and the real Registry probe journal.

use crate::{
    ConformanceClient, ConformanceResult, RuntimeControl, RuntimeControlRequest,
    RuntimeFaultObservation,
};
use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol::{ClientFrame, FrameId, OperationAnswer, OperationRequest, ServerFrame};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, WebSocket};
use serde_json::Value;
use std::time::Duration;

pub(crate) async fn control(
    client: &ConformanceClient,
    address: collaboration_central_core::ObjectAddress,
    command: RuntimeControl,
) -> ConformanceResult<RuntimeFaultObservation> {
    Ok(client
        .with_owner_access(
            client
                .client
                .post(format!("{}/admin/v1/conformance/runtime", client.base_url)),
        )?
        .json(&RuntimeControlRequest { address, command })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
pub(crate) async fn operation(
    socket: &mut WebSocket,
    body: OperationRequest<BoardConsumerProtocol>,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    send(socket, frame, body).await?;
    receive(socket, frame).await
}
pub(crate) async fn send(
    socket: &mut WebSocket,
    frame: FrameId,
    body: OperationRequest<BoardConsumerProtocol>,
) -> ConformanceResult<()> {
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request { frame, body })?
                .into(),
        ))
        .await?;
    Ok(())
}
pub(crate) async fn receive(
    socket: &mut WebSocket,
    frame: FrameId,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    let next = tokio::time::timeout(Duration::from_secs(10), socket.next())
        .await?
        .ok_or("composed session ended")??;
    let bytes = match next {
        Message::Binary(bytes) => bytes.to_vec(),
        Message::Text(text) => text.into_bytes(),
        other => return Err(format!("composed request received {other:?}").into()),
    };
    match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
        ServerFrame::Answer {
            frame: actual,
            body,
        } if actual == frame => Ok(body),
        _ => Err("composed request lost its exact requesting frame".into()),
    }
}
pub(crate) async fn copy_probe(
    client: &ConformanceClient,
    request: Value,
) -> ConformanceResult<Value> {
    let post = |route: &'static str, body: Value| async move {
        Ok::<Value, Box<dyn std::error::Error + Send + Sync>>(
            client
                .with_owner_access(
                    client
                        .client
                        .post(format!("{}/admin/v1/conformance/{route}", client.base_url)),
                )?
                .json(&body)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?,
        )
    };
    let mut reply = post(
        "copy-call",
        serde_json::json!({
            "attempt":durable_steps::Uuid::now_v7(),
            "copy":"019f0000-0000-7000-8000-000000000010", "request":request,
        }),
    )
    .await?;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match reply.get("kind").and_then(Value::as_str) {
                Some("completed") => {
                    return reply
                        .get("value")
                        .cloned()
                        .ok_or_else(|| "copy probe omitted its typed result".into());
                }
                Some("pending") => {
                    let operation: durable_steps::OperationId = serde_json::from_value(
                        reply
                            .pointer("/value/operation")
                            .cloned()
                            .ok_or("copy probe omitted its operation identity")?,
                    )?;
                    reply = post("copy-status", serde_json::to_value(operation)?).await?;
                }
                _ => return Err(format!("copy probe did not complete: {reply}").into()),
            }
        }
    })
    .await?
}
