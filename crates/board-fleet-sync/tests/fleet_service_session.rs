#![allow(clippy::unwrap_used)]

mod support;

use board_fleet_sync::{AccessServiceToken, ReqwestSessionTransport};
use fleet_sync_lease::{LeasePolicy, RegistryConfig};
use fleet_sync_protocol::{
    AuthRefusal, NoConsumer, OperationAnswer, OperationRequest, PeerId, ReceiverId, SchemaSupport,
};
use fleet_sync_service::{FleetService, ServiceOptions};
use fleet_sync_session::{
    ClientEvent, ConnectFailure, Endpoint, PeerCredential, SessionClient, SessionClientStore,
    SessionTransport,
};
use fleet_sync_tokio_runner::SessionPump;
use runtime_primitives::{Backoff, UuidV7Ids};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use support::{
    cut_proxy::CutProxy,
    session_service::{MemorySessionStore, SessionAuthenticator, SessionConsumer},
};
use tokio::{net::TcpListener, sync::watch, time::timeout};

const BOUND: Duration = Duration::from_secs(10);

#[tokio::test]
async fn fleet_service_resumes_the_same_session_after_a_socket_cut() {
    let directory = tempfile::tempdir().unwrap();
    let peer = PeerId::new(durable_steps::Uuid::now_v7()).unwrap();
    let receiver = ReceiverId::new(durable_steps::Uuid::now_v7()).unwrap();
    let consumer = Arc::new(SessionConsumer::default());
    let service = FleetService::<NoConsumer, _, _>::open(
        ServiceOptions {
            directory: directory.path().join("service"),
            delivery: None,
            receiver,
            registry: RegistryConfig {
                lease: LeasePolicy {
                    lease_duration: Duration::from_secs(600),
                    renew_interval: Duration::from_secs(30),
                },
                authoritative_receiver: receiver,
                release_unsent_for: Duration::from_secs(5),
                move_needs_renewal_within: None,
            },
            call_timeout: BOUND,
            socket_timeout: BOUND,
        },
        consumer.clone(),
        Arc::new(SessionAuthenticator { peer }),
    )
    .await
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let service_address = listener.local_addr().unwrap();
    let mut proxy = CutProxy::start(service_address).await.unwrap();
    let (stop_service, service_stopped) = watch::channel(false);
    let serving = tokio::spawn(service.serve(listener, axum::Router::new(), service_stopped));
    let store = MemorySessionStore::default();
    let transport = ReqwestSessionTransport::new(
        AccessServiceToken::new("fixture-id", "fixture-secret").unwrap(),
        BOUND,
        BOUND,
    )
    .unwrap();
    for (credential, expected) in [
        ("Bearer wrong-fixture-machine", AuthRefusal::Unknown),
        ("Bearer fixture-expired", AuthRefusal::Expired),
        ("Bearer fixture-revoked", AuthRefusal::Revoked),
        ("Bearer fixture-wrong-kind", AuthRefusal::WrongKind),
    ] {
        let refused = transport
            .connect(
                &Endpoint::new(&format!("ws://{service_address}/session")).unwrap(),
                &PeerCredential::new(credential).unwrap(),
            )
            .await;
        assert!(
            matches!(refused, Err(ConnectFailure::CredentialRejected(actual)) if actual == expected)
        );
    }
    let client = SessionClient::<_, _, NoConsumer>::load(
        transport,
        store.clone(),
        &UuidV7Ids,
        SchemaSupport { max_version: 1 },
        Backoff {
            initial: Duration::from_millis(1),
            max: Duration::from_millis(10),
            factor: 2,
        },
    )
    .await
    .unwrap();
    let original_session = client.state().session_id;
    let (requests, mut events, pump) = SessionPump::new(
        client,
        Endpoint::new(&format!("ws://{}/session", proxy.address)).unwrap(),
        PeerCredential::new("Bearer fixture-machine").unwrap(),
        BOUND,
    );
    let (stop_pump, pump_stopped) = watch::channel(false);
    let pumping = tokio::spawn(pump.run(pump_stopped));

    let first_headers = timeout(BOUND, proxy.upgrades.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        timeout(BOUND, events.recv()).await.unwrap(),
        Some(ClientEvent::Welcome { .. })
    ));
    assert!(matches!(
        requests
            .request(OperationRequest::Consumer(()))
            .await
            .unwrap(),
        OperationAnswer::Consumer(())
    ));
    proxy.cut.send(()).unwrap();
    assert!(matches!(
        timeout(BOUND, events.recv()).await.unwrap(),
        Some(ClientEvent::Disconnected { .. })
    ));
    let resumed_headers = timeout(BOUND, proxy.upgrades.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        timeout(BOUND, events.recv()).await.unwrap(),
        Some(ClientEvent::Welcome { .. })
    ));
    let resumed_answer = requests
        .request(OperationRequest::Consumer(()))
        .await
        .unwrap();
    let resumed_session = store.load().await.unwrap().unwrap().session_id;

    stop_pump.send_replace(true);
    timeout(BOUND, pumping).await.unwrap().unwrap().unwrap();
    proxy.stop.send_replace(true);
    timeout(BOUND, proxy.task).await.unwrap().unwrap().unwrap();
    stop_service.send_replace(true);
    timeout(BOUND, serving).await.unwrap().unwrap().unwrap();

    assert_eq!(first_headers, [true, true, true]);
    assert_eq!(resumed_headers, [true, true, true]);
    assert_eq!(resumed_session, original_session);
    assert!(matches!(resumed_answer, OperationAnswer::Consumer(())));
    assert_eq!(consumer.requests.load(Ordering::SeqCst), 2);
}
