//! The production driver's socket seam over a Turmoil TCP stream.
use central_link_protocol::MachineTokenText;
use collaboration_runtime::{
    LinkClosed, LinkInbound, LinkSocket, LinkTransport, LinkTransportError, TransportFuture,
};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{
    WebSocketStream, client_async,
    tungstenite::{Message, client::IntoClientRequest, http::header::AUTHORIZATION},
};
use turmoil::net::TcpStream;

pub struct TurmoilLinkTransport {
    central: &'static str,
    port: u16,
    path: String,
}

impl TurmoilLinkTransport {
    pub fn new(central: &'static str, port: u16, path: impl Into<String>) -> Self {
        Self {
            central,
            port,
            path: path.into(),
        }
    }
}

struct TurmoilLinkSocket(WebSocketStream<TcpStream>);

impl LinkTransport for TurmoilLinkTransport {
    fn connect<'call>(
        &'call self,
        token: &'call MachineTokenText,
    ) -> TransportFuture<'call, Result<Box<dyn LinkSocket>, LinkTransportError>> {
        Box::pin(async move {
            let stream = TcpStream::connect((self.central, self.port))
                .await
                .map_err(|_| LinkTransportError::Connect)?;
            let url = format!("ws://{}:{}{}", self.central, self.port, self.path);
            let mut request = url
                .into_client_request()
                .map_err(|_| LinkTransportError::Connect)?;
            let bearer = format!("Bearer {}", token.expose())
                .parse()
                .map_err(|_| LinkTransportError::Connect)?;
            request.headers_mut().insert(AUTHORIZATION, bearer);
            let (socket, _) = client_async(request, stream)
                .await
                .map_err(|_| LinkTransportError::Connect)?;
            Ok(Box::new(TurmoilLinkSocket(socket)) as Box<dyn LinkSocket>)
        })
    }
}

impl LinkSocket for TurmoilLinkSocket {
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
                match self.0.next().await {
                    Some(Ok(Message::Text(text))) => return LinkInbound::Text(text.to_string()),
                    Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
                    Some(Ok(Message::Binary(_) | Message::Close(_)) | Err(_)) | None => {
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
