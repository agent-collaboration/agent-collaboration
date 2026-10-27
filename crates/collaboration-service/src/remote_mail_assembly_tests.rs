//! Real runtime construction and Local intake need no welcomed remote carrier.
use super::*;
use crate::MailDeliveryRuntime;
use board_fleet_sync::{AccessServiceToken, MachineSessionStore, ReqwestSessionTransport};
use board_sync_protocol::BoardConsumerProtocol;
use collaboration_runtime::MailDeliverySession;
use fleet_sync_protocol::{PeerId, SchemaSupport};
use fleet_sync_session::{Endpoint, PeerCredential, SessionClient};
use fleet_sync_tokio_runner::SessionPump;
use runtime_primitives::{Backoff, UuidV7Ids};
use std::{path::Path, time::Duration};
use tokio::sync::watch;

#[path = "remote_direct_send_tests.rs"]
mod direct_send;
#[path = "project_mail_admission_tests.rs"]
mod project_admission;

async fn unwelcomed_session(directory: &Path, peer: PeerId) -> MailDeliverySession {
    let bound = Duration::from_secs(5);
    let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
        ReqwestSessionTransport::new(
            AccessServiceToken::new("assembly-id", "assembly-secret").unwrap(),
            bound,
            bound,
        )
        .unwrap(),
        MachineSessionStore::open(directory).await.unwrap(),
        &UuidV7Ids,
        SchemaSupport { max_version: 1 },
        Backoff {
            initial: bound,
            max: bound,
            factor: 2,
        },
    )
    .await
    .unwrap();
    let (requests, _events, _pump) = SessionPump::new(
        client,
        Endpoint::new("ws://127.0.0.1:1/session").unwrap(),
        PeerCredential::new("Bearer assembly-fixture").unwrap(),
        bound,
    );
    // No pump is activated and no socket is opened. The real remote request
    // handle is closed; any accidental dependency on central cannot pass.
    let (_, welcomed) = watch::channel(false);
    MailDeliverySession {
        requests,
        welcomed,
        peer,
        relay: PeerId::new(uuid::Uuid::now_v7()).unwrap(),
    }
}

#[tokio::test]
async fn mismatched_session_peer_refuses_assembly_before_any_mail_is_admitted() {
    let runtime = Runtime::new().await;
    let directory = tempfile::tempdir().unwrap();
    let peer = runtime.fixture.host.config.local.owner;
    let mismatched = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let session = unwelcomed_session(directory.path(), mismatched).await;
    let result = MailDeliveryRuntime::new(peer, runtime.local_mail_props(), Some(session));
    assert!(
        matches!(result, Err(error) if error.to_string().contains("installation peer")),
        "assembly accepted a different session peer"
    );
    // A matching, still-unwelcomed session is a constructive assembly control.
    let session = unwelcomed_session(directory.path(), peer).await;
    let mail = MailDeliveryRuntime::new(peer, runtime.local_mail_props(), Some(session)).unwrap();
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn local_send_completes_while_the_attached_remote_session_is_unwelcomed_and_closed() {
    let runtime = Runtime::new().await;
    let directory = tempfile::tempdir().unwrap();
    let peer = runtime.fixture.host.config.local.owner;
    let session = unwelcomed_session(directory.path(), peer).await;
    assert!(!*session.welcomed.borrow());
    let mail = MailDeliveryRuntime::new(peer, runtime.local_mail_props(), Some(session)).unwrap();
    let draft = runtime.fixture.draft();
    let item = runtime.fixture.item(&draft).id();
    runtime.client.script(item, vec![accepted()]);
    let answer = tokio::time::timeout(
        Duration::from_secs(5),
        mail.send_local(draft, "unwelcomed Local recipient".into()),
    )
    .await
    .expect("Local intake waited for Welcome")
    .unwrap();
    assert_eq!(answer.receipt.outcome, DeliveryOutcome::Started);
    assert_eq!(runtime.client.calls(item), 1);
    assert!(
        runtime
            .fixture
            .owner
            .message_send_answer(&item.as_uuid().to_string().try_into().unwrap(),)
            .await
            .unwrap()
            .unwrap()
            .result
            .is_some()
    );
    drop(mail);
    runtime.close().await;
}
