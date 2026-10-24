//! The same actual authenticated socket/frame transport serves Native and Worker oracles.
use board_sync_protocol::*;
use collaboration_central_conformance::{ConformanceResult, machine_assertion};
use fleet_sync_protocol::*;
use futures_util::{SinkExt, StreamExt};
use message_board::ProjectId;
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use std::time::Duration;

pub async fn connect(base: &str, credential: &str) -> ConformanceResult<WebSocket> {
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
    let session_id = SessionId::new(durable_steps::Uuid::now_v7())?;
    send(
        &mut socket,
        ClientFrame::Hello {
            session_id,
            schema_support: SchemaSupport { max_version: 1 },
        },
    )
    .await?;
    if !matches!(next(&mut socket).await?,ServerFrame::Welcome {session_id:actual,..} if actual==session_id)
    {
        return Err("forwarding connection lost its exact Welcome session identity".into());
    }
    Ok(socket)
}

pub async fn send(
    socket: &mut WebSocket,
    frame: ClientFrame<BoardConsumerProtocol>,
) -> ConformanceResult<()> {
    socket
        .send(Message::Binary(serde_json::to_vec(&frame)?.into()))
        .await?;
    Ok(())
}

pub async fn send_forward(
    socket: &mut WebSocket,
    project: ProjectId,
    post: ForwardedPost,
) -> ConformanceResult<FrameId> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    send(
        socket,
        ClientFrame::Request {
            frame,
            body: OperationRequest::Consumer(BoardConsumerRequest::ForwardedPost { project, post }),
        },
    )
    .await?;
    Ok(frame)
}

pub async fn next(socket: &mut WebSocket) -> ConformanceResult<ServerFrame<BoardConsumerProtocol>> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or("forwarding socket ended before its next frame")??;
            let frame = match message {
                Message::Text(text) => serde_json::from_str(&text)?,
                Message::Binary(bytes) => serde_json::from_slice(&bytes)?,
                other => return Err(format!("forwarding socket received {other:?}").into()),
            };
            if !matches!(&frame, ServerFrame::Notice { .. }) {
                return Ok(frame);
            }
        }
    })
    .await?
}

pub async fn close(socket: WebSocket) -> ConformanceResult<()> {
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    Ok(())
}
