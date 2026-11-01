//! Real HTTP upgrades prove startup waiting and the Authorization value on each reconnect.
use std::time::Duration;

use axum::{
    Router,
    extract::{State, WebSocketUpgrade},
    http::HeaderMap,
    response::Response,
    routing::get,
};
use futures_util::FutureExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::super::login_clock::LoginFuture;
use super::*;

struct ControlledClock;
impl FleetLoginClock for ControlledClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(0)
    }
    fn wait_until(&self, _: SyncTime) -> LoginFuture<'_, Result<(), ()>> {
        Box::pin(std::future::pending())
    }
}
async fn upgrade(
    State(observed): State<mpsc::UnboundedSender<String>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let _sent = observed.send(authorization);
    upgrade.on_upgrade(async |mut socket| {
        while let Some(Ok(message)) = socket.recv().await {
            if matches!(message, axum::extract::ws::Message::Close(_)) {
                break;
            }
        }
    })
}

#[tokio::test]
async fn fleet_machine_login_real_transport_waits_for_login_and_reconnects_with_latest_token_only()
{
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let (observed, mut authorizations) = mpsc::unbounded_channel();
    let router = Router::new()
        .route("/session", get(upgrade))
        .with_state(observed);
    let stop = CancellationToken::new();
    let shutdown = stop.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    });
    let (tokens, source) = MachineTokenSource::channel(Arc::new(ControlledClock));
    let transport = HostMachineSessionTransport::DeviceCode {
        transport: ReqwestSessionTransport::new(
            board_fleet_sync::AccessServiceToken::NoEdgeCredentials,
            Duration::from_secs(30),
            Duration::from_secs(30),
        )
        .unwrap(),
        source: source.clone(),
    };
    let endpoint = Endpoint::new(&format!("ws://{address}/session")).unwrap();
    let placeholder = PeerCredential::new("host-token-source").unwrap();
    let enrollment: EnrollmentId = "019f0000-0000-7000-8000-000000000001"
        .to_owned()
        .try_into()
        .unwrap();
    let connecting = transport.connect(&endpoint, &placeholder);
    tokio::pin!(connecting);
    assert!(connecting.as_mut().now_or_never().is_none());
    assert!(matches!(
        authorizations.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));

    tokens.send_replace(MachineAccessToken::Granted {
        enrollment_id: enrollment.clone(),
        token: SecretString::new("first-access-canary"),
        expires_at: SyncTime::from_unix_millis(10_000),
    });
    let first = connecting.await.unwrap();
    assert_eq!(
        authorizations.recv().await.unwrap(),
        "Bearer first-access-canary"
    );
    first.close(CloseReason::SessionEnded).await;
    tokens.send_replace(MachineAccessToken::Granted {
        enrollment_id: enrollment.clone(),
        token: SecretString::new("latest-access-canary"),
        expires_at: SyncTime::from_unix_millis(10_000),
    });

    let mut second = transport.connect(&endpoint, &placeholder).await.unwrap();

    assert_eq!(
        authorizations.recv().await.unwrap(),
        "Bearer latest-access-canary"
    );
    {
        let receiving = second.receive();
        tokio::pin!(receiving);
        assert!(receiving.as_mut().now_or_never().is_none());
        tokens.send_replace(MachineAccessToken::NotLoggedIn);
        assert_eq!(receiving.await, Some(Err(ConnectionClosed::Transport)));
    }
    // The pump owns closing/reconnecting after the returned transport ending.
    second.close(CloseReason::ConnectionReplaced).await;
    stop.cancel();
    server.await.unwrap().unwrap();
}
