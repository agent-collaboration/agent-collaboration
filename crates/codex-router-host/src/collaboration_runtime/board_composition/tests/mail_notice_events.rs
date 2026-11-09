//! SDK-generated hints cross the real pump and the Host's existing event owner.
use super::super::fleet_service::{BOUND, TestFleet};
use super::*;
use fleet_sync_protocol::{GateAnswer, OperationAnswer, OperationRequest};
use fleet_sync_session::{
    Bytes, ConnectFailure, ConnectionClosed, PeerCredential, SessionClientStore, SessionTransport,
    TransportConnection,
};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;

#[path = "mail_notice_ledger.rs"]
mod ledger;
use ledger::{NoticeLedger, TestError};
#[path = "mail_notice_owner.rs"]
mod mail_owner;

#[path = "mail_notice_harness.rs"]
mod harness;
use harness::MailHarness;

#[path = "../machine_session_push_test.rs"]
mod pushed_mail;

#[path = "mail_session_transport.rs"]
mod transport;
use transport::NoticeTransport;
#[path = "mail_relay_items.rs"]
mod relay_items;
#[path = "mail_relay_paths.rs"]
mod relay_paths;
#[path = "mail_status_catch_up.rs"]
mod status_catch_up;
#[path = "mail_status_reset_test.rs"]
mod status_reset;
#[path = "mail_submission_epochs.rs"]
mod submission_epochs;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn sdk_generated_notice_before_welcome_allows_a_real_durable_client_acknowledgement()
-> Result<(), TestError> {
    // Arrange a real central, real client resume file and a separate native
    // notice source using only public SDK rules, as bound by decision 688.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    tokio::fs::create_dir_all(&root).await?;
    let mut central =
        TestFleet::start(&directory.path().join("central"), GateAnswer::Admitted).await?;
    let ledger = NoticeLedger::open(&root, central.peer).await?;
    let inputs = central.inputs()?;
    let transport = NoticeTransport::new(
        ReqwestSessionTransport::new(inputs.access, BOUND, BOUND)?,
        ledger.clone(),
        true,
    );
    let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
        transport,
        MachineSessionStore::from_cache(std::sync::Arc::new(
            message_board_storage::FleetStore::open(
                &root.join(message_board_storage::FleetStore::FILE_NAME),
            )
            .await?
            .cache(),
        )),
        &UuidV7Ids,
        SchemaSupport { max_version: 1 },
        Backoff {
            initial: Duration::from_secs(1),
            max: BOUND,
            factor: 2,
        },
    )
    .await?;
    let (requests, mut events, pump) =
        SessionPump::new(client, inputs.endpoint, inputs.credential, BOUND);
    let (stop, stopped) = watch::channel(false);
    let pumping = tokio::spawn(pump.run(stopped));
    tokio::time::timeout(BOUND, central.auth.entered.notified()).await?;
    central.release()?;

    // Act: this event came from the real SDK client, so its notice_seen guard
    // permits an ACK rather than returning a Protocol refusal.
    let event = tokio::time::timeout(BOUND, events.recv())
        .await?
        .ok_or("notice event missing")?;
    let ClientEvent::Notice {
        session_id,
        position,
        body,
    } = event
    else {
        return Err("SDK did not produce the notice".into());
    };
    assert!(
        position == NoticePosition::new(1)
            && matches!(body,
        Notice::Consumer(ref notice) if matches!(notice.kind, board_sync_protocol::BoardNotice::LeaseSuperseded { .. }))
    );
    requests.acknowledge_notice(session_id, position).await?;
    let resumed = MachineSessionStore::from_cache(std::sync::Arc::new(
        message_board_storage::FleetStore::open(
            &root.join(message_board_storage::FleetStore::FILE_NAME),
        )
        .await?
        .cache(),
    ))
    .load()
    .await?
    .ok_or("resume state missing")?;
    assert_eq!(resumed.notice_cursor, NoticePosition::new(1));
    assert!(matches!(
        tokio::time::timeout(BOUND, events.recv()).await?,
        Some(ClientEvent::Welcome { .. })
    ));
    let answer = requests
        .request(OperationRequest::Consumer(
            board_sync_protocol::BoardConsumerRequest::Read {
                scope: board_sync_protocol::BoardReadScope::Notices,
                cursor: None,
            },
        ))
        .await?;
    assert!(matches!(
        answer,
        OperationAnswer::Consumer(board_sync_protocol::BoardConsumerAnswer::Read(_))
    ));

    stop.send_replace(true);
    tokio::time::timeout(BOUND, pumping).await???;
    drop(events);
    drop(requests);
    central.stop().await?;
    Arc::try_unwrap(ledger)
        .map_err(|_| io::Error::other("notice fixture retained"))?
        .close()
        .await?;
    Ok(())
}
