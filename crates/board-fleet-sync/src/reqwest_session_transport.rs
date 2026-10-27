use crate::{AccessServiceToken, session_credentials::sensitive_header};
use fleet_sync_protocol::{AuthRefusal, CloseReason};
use fleet_sync_session::{
    Bytes, ConnectFailure, ConnectionClosed, Endpoint, PeerCredential, SessionTransport,
    TransportConnection,
};
use futures_util::{SinkExt, StreamExt};
use reqwest::{Client, StatusCode};
use reqwest_websocket::{CloseCode, Message, RequestBuilderExt, WebSocket};
use std::time::Duration;
use tokio::time::timeout;

#[derive(Clone)]
pub struct ReqwestSessionTransport {
    client: Client,
    access: AccessServiceToken,
    connect_timeout: Duration,
    send_timeout: Duration,
}

impl ReqwestSessionTransport {
    pub fn new(
        access: AccessServiceToken,
        connect_timeout: Duration,
        send_timeout: Duration,
    ) -> Result<Self, ConnectFailure> {
        let client = Client::builder()
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(connect_timeout)
            .build()
            .map_err(|_| ConnectFailure::Unavailable)?;
        Ok(Self {
            client,
            access,
            connect_timeout,
            send_timeout,
        })
    }
}

pub struct ReqwestSessionConnection {
    socket: Option<WebSocket>,
    send_timeout: Duration,
}

impl SessionTransport for ReqwestSessionTransport {
    type Connection = ReqwestSessionConnection;

    async fn connect(
        &self,
        endpoint: &Endpoint,
        credential: &PeerCredential,
    ) -> Result<Self::Connection, ConnectFailure> {
        let endpoint =
            reqwest::Url::parse(endpoint.as_str()).map_err(|_| ConnectFailure::InvalidEndpoint)?;
        if !matches!(endpoint.scheme(), "ws" | "wss" | "http" | "https") {
            return Err(ConnectFailure::InvalidEndpoint);
        }
        let request = self.access.apply(self.client.get(endpoint)).header(
            reqwest::header::AUTHORIZATION,
            sensitive_header(credential.expose())?,
        );
        let response = timeout(self.connect_timeout, request.upgrade().send())
            .await
            .map_err(|_| ConnectFailure::Unavailable)?
            .map_err(|_| ConnectFailure::Unavailable)?;
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Err(ConnectFailure::CredentialRejected(auth_refusal(
                response.headers(),
            )));
        }
        let socket = timeout(self.connect_timeout, response.into_websocket())
            .await
            .map_err(|_| ConnectFailure::Unavailable)?
            .map_err(|_| ConnectFailure::Unavailable)?;
        Ok(ReqwestSessionConnection {
            socket: Some(socket),
            send_timeout: self.send_timeout,
        })
    }
}

fn auth_refusal(headers: &reqwest::header::HeaderMap) -> AuthRefusal {
    // C11's agreed unit-string header; importing the native adapter would add its socket stack.
    let mut reasons = headers.get_all("fleet-sync-auth-refusal").iter();
    let (Some(value), None) = (reasons.next(), reasons.next()) else {
        return AuthRefusal::Unknown;
    };
    let Ok(reason) = serde_json::from_slice::<String>(value.as_bytes()) else {
        return AuthRefusal::Unknown;
    };
    serde_json::from_value(serde_json::Value::String(reason)).unwrap_or(AuthRefusal::Unknown)
}

impl TransportConnection for ReqwestSessionConnection {
    async fn send(&mut self, frame: Bytes) -> Result<(), ConnectionClosed> {
        let Some(socket) = self.socket.as_mut() else {
            return Err(ConnectionClosed::Transport);
        };
        match timeout(self.send_timeout, socket.send(Message::Binary(frame))).await {
            Ok(Ok(())) => Ok(()),
            _ => {
                self.socket = None;
                Err(ConnectionClosed::Transport)
            }
        }
    }

    async fn receive(&mut self) -> Option<Result<Bytes, ConnectionClosed>> {
        let socket = self.socket.as_mut()?;
        loop {
            match socket.next().await {
                Some(Ok(Message::Binary(frame))) => return Some(Ok(frame)),
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Close { .. })) | None => {
                    self.socket = None;
                    return None;
                }
                Some(Ok(Message::Text(_))) => {
                    self.socket = None;
                    return Some(Err(ConnectionClosed::UnsupportedFrame));
                }
                Some(Err(_)) => {
                    self.socket = None;
                    return Some(Err(ConnectionClosed::Transport));
                }
            }
        }
    }

    async fn close(self, _reason: CloseReason) {
        if let Some(socket) = self.socket {
            let _ = timeout(self.send_timeout, socket.close(CloseCode::Normal, None)).await;
        }
    }
}
