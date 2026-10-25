//! SDK-generated hints cross the real pump and the Host's existing event owner.
#![allow(clippy::panic_in_result_fn)] // Assertions are test oracles; Result propagates real I/O failures.
use super::super::fleet_service::{BOUND, TestFleet};
use super::*;
use fleet_sync_protocol::{DeliveryStream, GateAnswer, OperationAnswer, OperationRequest};
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

#[tokio::test]
async fn pre_welcome_hint_is_deferred_then_admitted_with_both_welcome_streams_before_ack()
-> Result<(), TestError> {
    let mut harness = MailHarness::open(true).await?;
    // Actual client Notice precedes the gated Host Welcome: no admission or ACK.
    assert!(!*harness.welcomed.borrow());
    assert!(harness.mail.state().await?.is_none());
    assert_eq!(harness.ack().await?, NoticePosition::new(0));
    harness.release_welcome()?;
    // The real mail runner is held inactive so its exact durable obligations
    // remain inspectable. Fetch behavior is proved separately through central.
    let state = harness
        .wait_state(
            |state| {
                state["items"]["rerunRequested"] == true
                    && state["controls"]["active"]["cycle"] == 1
            },
            "deferred hint did not commit alongside both Welcome receive streams",
        )
        .await?;
    harness.wait_ack(NoticePosition::new(1)).await?;
    assert_eq!(state["items"]["active"]["cycle"], 1);
    assert_eq!(state["items"]["nextCycle"], 2);
    assert_eq!(state["controls"]["nextCycle"], 2);
    assert_eq!(state["controls"]["rerunRequested"], false);
    assert!(*harness.welcomed.borrow());
    harness.close().await
}

#[tokio::test]
async fn five_minute_pass_admits_both_streams_without_a_notice_and_waits_while_unwelcomed()
-> Result<(), TestError> {
    let mut harness = MailHarness::open(false).await?;
    // Keep Tokio from automatically jumping over native I/O while its clock is
    // paused. Only explicit advance below moves this test's elapsed time.
    tokio::time::pause();
    let clock_guard = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    tokio::time::advance(Duration::from_secs(5 * 60)).await;
    assert!(
        harness.mail.state().await?.is_none(),
        "unwelcomed timer admitted mail"
    );
    assert_eq!(harness.ack().await?, NoticePosition::new(0));
    harness.release_welcome()?;
    let state = harness
        .wait_state(
            |state| {
                state["items"]["active"]["cycle"] == 1 && state["controls"]["active"]["cycle"] == 1
            },
            "Welcome did not admit both receive streams",
        )
        .await?;
    assert_eq!(state["items"]["rerunRequested"], false);
    assert_eq!(state["controls"]["rerunRequested"], false);
    tokio::time::advance(Duration::from_secs(299)).await;
    let before = harness
        .mail
        .state()
        .await?
        .ok_or("Welcome state disappeared")?;
    assert_eq!(
        before["items"]["rerunRequested"], false,
        "receive timer fired early"
    );
    assert_eq!(before["controls"]["rerunRequested"], false);
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::time::resume();
    clock_guard.abort();
    let _ = clock_guard.await;
    let state = harness
        .wait_state(
            |state| {
                state["items"]["rerunRequested"] == true
                    && state["controls"]["rerunRequested"] == true
            },
            "five-minute branch did not commit both receive streams",
        )
        .await?;
    assert_eq!(
        state["items"]["nextCycle"], 2,
        "timer bypassed SDK cycle coalescing"
    );
    assert_eq!(state["controls"]["nextCycle"], 2);
    assert_eq!(
        harness.ack().await?,
        NoticePosition::new(0),
        "fixture unexpectedly supplied a notice"
    );
    harness.close().await
}

#[path = "mail_session_transport.rs"]
mod transport;
use transport::NoticeTransport;
#[path = "mail_relay_items.rs"]
mod relay_items;
#[path = "mail_relay_paths.rs"]
mod relay_paths;
#[path = "mail_status_catch_up.rs"]
mod status_catch_up;
#[path = "mail_submission_epochs.rs"]
mod submission_epochs;

#[tokio::test]
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
        MachineSessionStore::open(&root).await?,
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
    assert!(
        matches!(event, ClientEvent::Notice { position, body: Notice::Consumer(ref notice) }
        if position == NoticePosition::new(1) && matches!(notice.kind, board_sync_protocol::BoardNotice::MailWaiting { stream: DeliveryStream::Items }))
    );
    requests.acknowledge_notice(NoticePosition::new(1)).await?;
    let resumed = MachineSessionStore::open(&root)
        .await?
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
