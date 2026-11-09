use crate::consumer_fixture::*;
use durable_steps::CallFailure;
use fleet_protocol::*;
use fleet_session::*;
use fleet_session_client::SessionPump;
use fleet_websocket_transport::WebSocketTransport;
use runtime_primitives::{Backoff, UuidV7Ids};
use std::time::Duration;
use tokio::sync::watch;

#[tokio::test]
async fn a_request_during_transport_activation_is_also_not_sent() -> TestResult {
    let root = tempfile::tempdir()?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let address = listener.local_addr()?;
    let (accepted, acceptance) = tokio::sync::oneshot::channel();
    let (close, closed) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await?;
        let _ = accepted.send(());
        // Deliberately hold the real HTTP upgrade, before any session frame.
        let _ = closed.await;
        drop(socket);
        Ok::<_, std::io::Error>(())
    });
    let client = SessionClient::<_, _, SampleProtocol>::load(
        WebSocketTransport::default(),
        ConsumerStore::open(&root.path().join("client.db")).await?,
        &UuidV7Ids,
        SchemaSupport { max_version: 5 },
        Backoff {
            initial: Duration::from_millis(10),
            max: Duration::from_secs(1),
            factor: 2,
        },
    )
    .await?;
    let (requests, _events, pump) = SessionPump::new(
        client,
        Endpoint::new(&format!("ws://{address}/session"))?,
        PeerCredential::new("Bearer peer-4")?,
        Duration::from_millis(500),
    );
    let (stop, stopped) = watch::channel(false);
    let pumping = tokio::spawn(pump.run(stopped));
    tokio::time::timeout(Duration::from_secs(5), acceptance).await??;
    let answer = requests
        .request(OperationRequest::Consumer(SampleRequest::Echo {
            text: "before transport activation".into(),
        }))
        .await;
    stop.send(true)?;
    pumping.await??;
    let _ = close.send(());
    server.await??;
    assert!(
        matches!(answer, Err(CallFailure::NotSent { .. })),
        "a queued operation cannot have reached a transport that is still connecting"
    );
    Ok(())
}
