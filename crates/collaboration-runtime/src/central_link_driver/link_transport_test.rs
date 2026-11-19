//! The production WebSocket transport against a loopback server: it sends the
//! bearer token and reads central's lapsed-authority close as AuthorityLapsed.
use super::*;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};

/// Accepts one link, checks its bearer, sends one text frame and closes with
/// `code` and `reason`.
#[expect(
    clippy::result_large_err,
    reason = "tungstenite's accept_hdr_async callback signature fixes the error type."
)]
async fn central_closing_with(
    code: u16,
    reason: &'static str,
) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/link", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut bearer = String::new();
        let mut socket = tokio_tungstenite::accept_hdr_async(
            stream,
            |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                bearer = request
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_owned();
                Ok(response)
            },
        )
        .await
        .unwrap();
        socket.send(Message::text("frame")).await.unwrap();
        socket
            .close(Some(CloseFrame {
                code: CloseCode::from(code),
                reason: reason.into(),
            }))
            .await
            .unwrap();
        bearer
    });
    (url, server)
}

#[tokio::test]
async fn a_lapsed_authority_close_is_reported_as_authority_lapsed() {
    let (url, server) = central_closing_with(1008, "machine authority expired or revoked").await;
    let token: MachineTokenText = "fixture.machine.token".to_owned().try_into().unwrap();

    let mut socket = WebSocketLinkTransport::new(url)
        .connect(&token)
        .await
        .unwrap();
    let first = socket.next_inbound().await;
    let second = socket.next_inbound().await;

    assert_eq!(first, LinkInbound::Text("frame".to_owned()));
    assert_eq!(second, LinkInbound::Closed(LinkClosed::AuthorityLapsed));
    assert_eq!(server.await.unwrap(), "Bearer fixture.machine.token");
}

#[tokio::test]
async fn another_policy_close_is_not_authority_lapsed() {
    let (url, _server) = central_closing_with(1008, "invalid link frame").await;
    let token: MachineTokenText = "fixture.machine.token".to_owned().try_into().unwrap();

    let mut socket = WebSocketLinkTransport::new(url)
        .connect(&token)
        .await
        .unwrap();
    let _ = socket.next_inbound().await;
    let closed = socket.next_inbound().await;

    assert_eq!(closed, LinkInbound::Closed(LinkClosed::Other));
}
