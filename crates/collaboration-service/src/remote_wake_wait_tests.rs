//! The remote-wake wait reads the outbox once, then sleeps until the interaction store
//! commits or the wake's deadline; it never re-reads on a tick.
use super::native::Runtime;
use super::remote_wakes::{create_request, creation_mail, service};
use automation_storage::RemoteWakeMail;
use collaboration_contracts::RouterDeliveryTypes;
use collaboration_mail_protocol::{CarrierAcceptance, RemoteWakeRefusal, RemoteWakeResult};
use durable_steps::{AtomicStore, StoreError};
use fleet_sync_delivery::{Outbox, delivery_rollback};
use fleet_sync_protocol::{
    DeliveryItem, DeliveryStatus, ItemId, PeerId, StatusPublication, StatusVersion,
};
use std::{sync::Arc, sync::atomic::Ordering, time::Duration};

/// Bounds a wait the test expects to end on a commit or a deadline; it only stops a hang.
const HANG_GUARD: Duration = Duration::from_secs(10);

fn wall_now(runtime: &Runtime) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp_millis(runtime.fixture.host.clock.load(Ordering::SeqCst))
        .unwrap()
}

fn wake_mail(runtime: &Runtime, after: chrono::Duration) -> (RemoteWakeMail, ItemId) {
    let deadline = wall_now(runtime) + after;
    let mail = creation_mail(&create_request(deadline), deadline);
    let id = ItemId::new(uuid::Uuid::parse_str(mail.operation_id.as_str()).unwrap()).unwrap();
    (mail, id)
}

/// Puts the wake in the sender's outbox with no answer yet, as a sent remote wake is.
async fn enqueue_unanswered(source: &Runtime, mail: RemoteWakeMail) {
    let item = DeliveryItem::<RouterDeliveryTypes>::new(
        ItemId::new(uuid::Uuid::parse_str(mail.operation_id.as_str()).unwrap()).unwrap(),
        collaboration_mail_protocol::MailSender::HumanUser {
            installation: String::from(service(1)).try_into().unwrap(),
        },
        fleet_sync_protocol::Recipient {
            peer: PeerId::new(uuid::Uuid::now_v7()).unwrap(),
            address: serde_json::from_value(serde_json::to_value(mail.target).unwrap()).unwrap(),
        },
        Some(runtime_primitives::SyncTime::from_unix_millis(
            mail.valid_until_ms,
        )),
        board_sync_protocol::BoardDeliveryBody::Wake(mail.body),
    )
    .unwrap();
    let relay = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    let limits = source.fixture.host.config.local.limits;
    let binding = source.fixture.host.binding.clone();
    let submitter = fleet_sync_protocol::Submitter::Machine(source.fixture.host.config.local.owner);
    let observed_at = runtime_primitives::Clock::now(source.fixture.host.as_ref());
    source
        .fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                Outbox::<_, RouterDeliveryTypes, _>::new(scope, limits, binding.as_ref())
                    .enqueue(item, relay, submitter, observed_at)
                    .await
                    .map_err(delivery_rollback)?;
                Ok::<_, StoreError>(())
            })
        })
        .await
        .unwrap();
}

/// Commits the recipient's answer, as its status report does.
async fn commit_answer(source: &Runtime, id: ItemId, answer: RemoteWakeResult) {
    let limits = source.fixture.host.config.local.limits;
    let binding = source.fixture.host.binding.clone();
    let observed_at = runtime_primitives::Clock::now(source.fixture.host.as_ref());
    source
        .fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                Outbox::<_, RouterDeliveryTypes, _>::new(scope, limits, binding.as_ref())
                    .publish_status(
                        StatusPublication {
                            item: id,
                            version: StatusVersion::new(1),
                            cancel: None,
                            status: DeliveryStatus::Delivered {
                                acceptance: CarrierAcceptance::Wake(Box::new(answer)),
                            },
                        },
                        observed_at,
                    )
                    .await
                    .map_err(delivery_rollback)?;
                Ok::<_, StoreError>(())
            })
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn a_remote_wake_wait_answers_the_status_commit_that_lands_after_its_empty_read() {
    let source = Runtime::new().await;
    let (mail, id) = wake_mail(&source, chrono::Duration::hours(1));
    let deadline_ms = mail.valid_until_ms;
    enqueue_unanswered(&source, mail).await;
    let runtime = Arc::new(source.local_mail_runtime());
    let observation = runtime.remote_wake_wait_observation();
    let mut gate = observation.hold_before_next_sleep();
    let waiter = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.await_remote_wake_answer(id, deadline_ms).await }
    });
    gate.reached().await;
    let answer = RemoteWakeResult::Refused {
        reason: RemoteWakeRefusal::ResourceNotFound,
        mutation: collaboration_mail_protocol::RemoteWakeMutationState::None,
    };

    // The answer commits after the wait's empty read and before it starts waiting.
    commit_answer(&source, id, answer.clone()).await;
    gate.release();
    let answered = tokio::time::timeout(HANG_GUARD, waiter)
        .await
        .expect("the commit, not the hour-long deadline, ends the wait")
        .unwrap()
        .unwrap();

    assert_eq!(
        serde_json::to_value(answered).unwrap(),
        serde_json::to_value(answer).unwrap()
    );
    let snapshot = observation.snapshot();
    assert_eq!(snapshot.reads, vec!["remote_wake.outbox_status"; 2]);
    assert_eq!(snapshot.sleeps, 1);
    drop(runtime);
    source.close().await;
}

// The clock is paused only once the waiter is parked, with no store read in flight.
#[tokio::test]
async fn a_remote_wake_wait_with_nothing_committed_reads_once_then_once_at_its_deadline() {
    let source = Runtime::new().await;
    let (mail, id) = wake_mail(&source, chrono::Duration::seconds(30));
    let deadline_ms = mail.valid_until_ms;
    enqueue_unanswered(&source, mail).await;
    let runtime = Arc::new(source.local_mail_runtime());
    let observation = runtime.remote_wake_wait_observation();
    let mut gate = observation.hold_before_next_sleep();
    let waiter = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.await_remote_wake_answer(id, deadline_ms).await }
    });
    gate.reached().await;
    tokio::time::pause();
    let parked_at = tokio::time::Instant::now();
    // The wall clock reaches the deadline as Tokio's timer does.
    source
        .fixture
        .host
        .clock
        .store(deadline_ms, Ordering::SeqCst);

    gate.release();
    let waited = waiter.await.unwrap();

    assert!(
        matches!(waited, Err(RemoteWakeRefusal::DeadlineExpired)),
        "{waited:?}"
    );
    let slept = tokio::time::Instant::now().duration_since(parked_at);
    assert!(
        slept > Duration::from_secs(29) && slept < Duration::from_secs(31),
        "the wait slept until its deadline: {slept:?}"
    );
    let snapshot = observation.snapshot();
    assert_eq!(
        snapshot.reads,
        vec!["remote_wake.outbox_status"; 2],
        "one read before the wait and one at the deadline, none in between"
    );
    assert_eq!(snapshot.sleeps, 1);
    drop(runtime);
    source.close().await;
}
