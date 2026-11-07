//! Fixture Access assertions around the real websocket; SessionClient and pump stay unchanged.
use collaboration_central_conformance::{ConformanceResult, machine_assertion};
use fleet_protocol::CloseReason;
use fleet_session::{
    Bytes, ConnectFailure, ConnectionClosed, Endpoint, PeerCredential, SessionTransport,
    TransportConnection,
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
#[derive(Clone, Default)]
pub(super) struct FixtureTransport {
    pub sends: Arc<AtomicUsize>,
}
pub(super) struct FixtureConnection {
    socket: WebSocket,
    sends: Arc<AtomicUsize>,
}
impl SessionTransport for FixtureTransport {
    type Connection = FixtureConnection;
    async fn connect(
        &self,
        endpoint: &Endpoint,
        credential: &PeerCredential,
    ) -> Result<Self::Connection, ConnectFailure> {
        let assertion = machine_assertion(credential.expose())
            .map_err(|_| ConnectFailure::InvalidCredential)?;
        let socket = reqwest::Client::new()
            .get(endpoint.as_str())
            .header("Cf-Access-Jwt-Assertion", assertion)
            .bearer_auth(credential.expose())
            .upgrade()
            .send()
            .await
            .map_err(|_| ConnectFailure::Unavailable)?
            .into_websocket()
            .await
            .map_err(|_| ConnectFailure::Unavailable)?;
        Ok(FixtureConnection {
            socket,
            sends: self.sends.clone(),
        })
    }
}
impl TransportConnection for FixtureConnection {
    async fn send(&mut self, frame: Bytes) -> Result<(), ConnectionClosed> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        self.socket
            .send(Message::Binary(frame))
            .await
            .map_err(|_| ConnectionClosed::Transport)
    }
    async fn receive(&mut self) -> Option<Result<Bytes, ConnectionClosed>> {
        loop {
            match self.socket.next().await? {
                Ok(Message::Binary(bytes)) => return Some(Ok(bytes)),
                Ok(Message::Text(text)) => return Some(Ok(text.into_bytes().into())),
                Ok(Message::Ping(_) | Message::Pong(_)) => {}
                Ok(Message::Close { .. }) => return None,
                Err(_) => return Some(Err(ConnectionClosed::Transport)),
            }
        }
    }
    async fn close(self, _: CloseReason) {
        let _ = self
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
    }
}
pub(super) async fn enroll(
    base: &str,
    service: &str,
    peer: fleet_protocol::PeerId,
    credential: &str,
) -> ConformanceResult<serde_json::Value> {
    let hash = ring::digest::digest(&ring::digest::SHA256, credential.as_bytes())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(reqwest::Client::new().post(format!("{base}/admin/v1/machines/enroll"))
       .header("Cf-Access-Jwt-Assertion",collaboration_central_conformance::owner_assertion()?)
       .json(&serde_json::json!({"kind":"enrollMachine","value":{"installation":service,"peer":peer,"label":credential,
        "access_client_id":collaboration_central_conformance::access_client_id(credential)?,"credential_hash":hash,"expires_at":"2033-05-18T03:33:20Z"}}))
       .send().await?.error_for_status()?.json().await?)
}

pub(super) async fn adopted_writer(
    path: &std::path::Path,
    peer: fleet_protocol::PeerId,
) -> ConformanceResult<fleet_project_writer::ReplicatedDatabase<board_fleet_sync::BoardProjectSchema>>
{
    use fleet_project_writer::ReplicatedSchema;
    use sqlx_core::connection::{ConnectOptions, Connection};
    let schema = board_fleet_sync::BoardProjectSchema::new()?;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .connect()
        .await?;
    sqlx_core::migrate::Migrator::with_migrations(schema.migrations().to_vec())
        .run_direct(None, &mut connection, false)
        .await?;
    connection.close().await?;
    Ok(fleet_project_writer::ReplicatedDatabase::adopt(
        path,
        schema,
        peer,
        fleet_project_lease::LeasePolicy {
            lease_duration: std::time::Duration::from_secs(14400),
            renew_interval: std::time::Duration::from_secs(900),
        },
        fleet_project_writer::WriterOptions::default(),
    )
    .await?)
}
