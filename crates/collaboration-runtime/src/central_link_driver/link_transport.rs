//! The socket under the Router's central link. Production connects with
//! tokio-tungstenite; tests and the Turmoil simulation supply their own.
use central_link_protocol::MachineTokenText;
use futures_util::{SinkExt, StreamExt};
use std::{future::Future, pin::Pin};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};

pub type TransportFuture<'call, T> = Pin<Box<dyn Future<Output = T> + Send + 'call>>;

#[derive(Debug, thiserror::Error)]
pub enum LinkTransportError {
    #[error("central link connection failed")]
    Connect,
    #[error("central link send failed")]
    Send,
}

/// Why central closed the link.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkClosed {
    /// 1008 "machine authority expired or revoked": the connection's deadline
    /// passed or its enrollment ended; only a fresh token connects again.
    AuthorityLapsed,
    /// Any other close, a failure or the end of the stream.
    Other,
}

/// What the next read from the socket gives.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkInbound {
    Text(String),
    Closed(LinkClosed),
}

/// The close code and reason both hosts send when a connection's authority lapsed.
pub const AUTHORITY_LAPSED_CLOSE: (u16, &str) = (1008, "machine authority expired or revoked");

/// One open link socket carrying text frames.
pub trait LinkSocket: Send + Sync {
    fn send_text(&mut self, text: String) -> TransportFuture<'_, Result<(), LinkTransportError>>;
    /// The next text frame, or how the socket closed.
    fn next_inbound(&mut self) -> TransportFuture<'_, LinkInbound>;
    fn close(&mut self) -> TransportFuture<'_, ()>;
}

/// Opens a link socket to central's `/v1/organizations/{org}/link` with the
/// machine token as the bearer.
pub trait LinkTransport: Send + Sync {
    fn connect<'call>(
        &'call self,
        token: &'call MachineTokenText,
    ) -> TransportFuture<'call, Result<Box<dyn LinkSocket>, LinkTransportError>>;
}

/// Where the Router's sign-in machine token stands (B3 owns the policy).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenAvailability {
    /// A token to connect or renew with.
    Ready(MachineTokenText),
    /// The source is refreshing at sign-in; try again when it changes.
    RefreshPending,
    /// The enrollment needs a new code from a person; the link stops retrying
    /// and reports "not enrolled; new code needed" until the source changes.
    NeedsNewCode,
}

/// The Router's machine token, supplied by the sign-in machine-login lane (B3).
/// An unconfigured Host reports not enrolled while outbox writes wait.
pub trait MachineTokenSource: Send + Sync {
    /// The current state, read without waiting.
    fn availability(&self) -> TokenAvailability;
    /// Resolves when the availability changes, e.g. after a refresh at sign-in.
    /// The driver is the one waiter and drops this future whenever another
    /// event wins its select, so it must be cancel-safe: a change made while no
    /// call was waiting resolves the next call (a `watch` receiver held by the
    /// source does this). No time-based polling: it waits on the source.
    fn changed(&self) -> TransportFuture<'_, ()>;
    /// Every RenewMachineToken result, so the source can refresh once more or
    /// move to NeedsNewCode (B3 M4).
    fn report_renewal(&self, outcome: &central_link_protocol::TokenRenewalResult);
}

/// The production transport: a WebSocket to the configured link URL.
pub struct WebSocketLinkTransport {
    link_url: String,
}

impl WebSocketLinkTransport {
    #[must_use]
    pub fn new(link_url: String) -> Self {
        Self { link_url }
    }
}

type Stream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct WebSocketLink(Stream);

impl LinkTransport for WebSocketLinkTransport {
    fn connect<'call>(
        &'call self,
        token: &'call MachineTokenText,
    ) -> TransportFuture<'call, Result<Box<dyn LinkSocket>, LinkTransportError>> {
        Box::pin(async move {
            let mut request = self
                .link_url
                .as_str()
                .into_client_request()
                .map_err(|_| LinkTransportError::Connect)?;
            let bearer = HeaderValue::from_str(&format!("Bearer {}", token.expose()))
                .map_err(|_| LinkTransportError::Connect)?;
            request.headers_mut().insert("authorization", bearer);
            let (stream, _) = tokio_tungstenite::connect_async(request)
                .await
                .map_err(|_| LinkTransportError::Connect)?;
            Ok(Box::new(WebSocketLink(stream)) as Box<dyn LinkSocket>)
        })
    }
}

impl LinkSocket for WebSocketLink {
    fn send_text(&mut self, text: String) -> TransportFuture<'_, Result<(), LinkTransportError>> {
        Box::pin(async move {
            self.0
                .send(Message::text(text))
                .await
                .map_err(|_| LinkTransportError::Send)
        })
    }

    fn next_inbound(&mut self) -> TransportFuture<'_, LinkInbound> {
        Box::pin(async move {
            loop {
                let Some(message) = self.0.next().await else {
                    return LinkInbound::Closed(LinkClosed::Other);
                };
                match message {
                    Ok(Message::Text(text)) => return LinkInbound::Text(text.to_string()),
                    Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
                    Ok(Message::Close(Some(frame)))
                        if (u16::from(frame.code), frame.reason.as_str())
                            == AUTHORITY_LAPSED_CLOSE =>
                    {
                        return LinkInbound::Closed(LinkClosed::AuthorityLapsed);
                    }
                    Ok(Message::Binary(_) | Message::Close(_)) | Err(_) => {
                        return LinkInbound::Closed(LinkClosed::Other);
                    }
                }
            }
        })
    }

    fn close(&mut self) -> TransportFuture<'_, ()> {
        Box::pin(async move {
            let _ = self.0.close(None).await;
        })
    }
}

#[cfg(test)]
#[path = "link_transport_test.rs"]
mod tests;
