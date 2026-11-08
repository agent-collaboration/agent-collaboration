use crate::consumer_fixture::{FixtureAuthenticator, SampleProtocol, TestResult};
use crate::support::{ServiceProcess, server_frame};
use fleet_protocol::*;
use fleet_session::{PeerAuthenticator, TransportConnection};
use fleet_websocket_transport::WebSocketConnection;
use serde_json::Value;
use std::path::Path;

pub async fn start_service(root: &Path) -> TestResult<ServiceProcess> {
    ServiceProcess::start_as(root, new_receiver().await?.as_uuid().as_u128()).await
}

pub async fn attach_peer(
    service: &ServiceProcess,
    peer: PeerId,
    support: u64,
) -> TestResult<WebSocketConnection> {
    let mut socket = service
        .connect(
            "session",
            &format!("Bearer peer-{}", peer.as_uuid().as_u128()),
        )
        .await?;
    let session_id = SessionId::new(durable_steps::Uuid::now_v7())?;
    let hello: ClientFrame<SampleProtocol> = ClientFrame::Hello {
        capabilities: None,
        session_id,
        schema_support: SchemaSupport {
            max_version: support,
        },
    };
    socket.send(serde_json::to_vec(&hello)?.into()).await?;
    match server_frame(&mut socket).await? {
        ServerFrame::Welcome {
            session_id: returned,
            ..
        } if returned == session_id => Ok(socket),
        _ => Err("matching Welcome absent".into()),
    }
}

pub async fn receive_json(socket: &mut WebSocketConnection) -> TestResult<Value> {
    loop {
        let bytes = tokio::time::timeout(std::time::Duration::from_secs(15), socket.receive())
            .await?
            .ok_or("session closed without answer")??;
        let frame: Value = serde_json::from_slice(&bytes)?;
        if frame.get("kind").and_then(Value::as_str) != Some("notice") {
            return Ok(frame);
        }
    }
}

async fn new_fixture_uuid() -> TestResult<durable_steps::Uuid> {
    // Reuse the consumer's public credential mapping instead of duplicating its namespace.
    let raw = durable_steps::Uuid::now_v7();
    let authenticated = FixtureAuthenticator::default()
        .verify(format!("Bearer peer-{}", raw.as_u128()))
        .await
        .map_err(|reason| format!("fixture identity refused: {reason:?}"))?;
    Ok(authenticated.0.peer.as_uuid())
}

pub async fn new_peer() -> TestResult<PeerId> {
    Ok(PeerId::new(new_fixture_uuid().await?)?)
}

pub async fn new_receiver() -> TestResult<ReceiverId> {
    Ok(ReceiverId::new(new_fixture_uuid().await?)?)
}
