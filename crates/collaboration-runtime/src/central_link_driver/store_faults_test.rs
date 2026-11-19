//! The step-8 Verifier's three scenarios (Lead A, 2026-11-18): a store error
//! must never strand an accepted write while the socket stays healthy.
use super::super::driver_test_support::*;
use super::super::store_faults::{FaultPlan, FaultyOutbox};
use super::super::*;
use crate::outbox_store::NewOutboxItem;
use central_link_client::{LinkCalls, MemberOutcome, RequestStatus};
use central_link_protocol::{DataFrame, FrameDirection, LinkFrame, LinkRequest, decode_text};
use runtime_primitives::Clock;
use std::time::Duration;

async fn faulty_store(root: &tempfile::TempDir) -> (Arc<Mutex<dyn OutboxStorage>>, FaultPlan) {
    let inner = open_outbox(&root.path().join("machine_local.sqlite"))
        .await
        .unwrap();
    let plan = FaultPlan::default();
    let store = FaultyOutbox {
        inner: Box::new(inner),
        plan: plan.clone(),
    };
    (Arc::new(Mutex::new(store)), plan)
}

#[tokio::test]
async fn a_failed_local_status_read_is_unavailable() {
    let root = tempfile::tempdir().unwrap();
    let (store, plan) = faulty_store(&root).await;
    let harness = Harness::start_with_store(root, clock_at(MARK - 20_000), store);
    plan.fail_once("request_status");
    let status = harness
        .calls
        .request_status(caller(), status_query(&uuid_v7(29)))
        .await;
    assert_eq!(status, RequestStatus::StatusUnavailable);
    assert!(
        plan.pending().is_empty(),
        "the status read hit the injected error"
    );
    harness.stop().await;
}

/// The batch central has received so far, if any; heartbeats are answered.
/// Never blocks: a stranded write shows as None, not as a hang.
async fn batch_now(harness: &Harness, central: &mut CentralEnd) -> Option<Vec<LinkRequest>> {
    for _ in 0..2 {
        let _ = harness.calls.link_status().await;
    }
    let mut batch = None;
    while let Ok(text) = central.from_router.try_recv() {
        match decode_text(&text, FrameDirection::ToCentral).unwrap() {
            LinkFrame::Ping => central.send(LinkFrame::Pong),
            LinkFrame::Data(DataFrame::Batch { requests }) => {
                batch = Some(requests.requests().to_vec());
            }
            other => panic!("unexpected frame: {other:?}"),
        }
    }
    batch
}

fn answer_all(central: &CentralEnd, members: &[LinkRequest]) {
    for member in members {
        central.send(LinkFrame::Data(DataFrame::Reply {
            correlation_id: member.correlation_id().clone(),
            result: Box::new(reply()),
        }));
    }
}

fn advance_to_next_mark(harness: &Harness) {
    let now = harness.clock.now().unix_millis();
    let next = (now.div_euclid(60_000) + 1) * 60_000;
    harness
        .clock
        .advance(Duration::from_millis(u64::try_from(next - now).unwrap()))
        .unwrap();
}

#[tokio::test]
async fn a_failed_restore_read_does_not_lose_the_waiting_write() {
    let root = tempfile::tempdir().unwrap();
    let (store, plan) = faulty_store(&root).await;
    let id = uuid_v7(30);
    store
        .lock()
        .await
        .insert(
            &NewOutboxItem {
                request: id.parse().unwrap(),
                caller: caller(),
                payload: queued_thread_post(&id),
                flush_at: SyncTime::from_unix_millis(MARK),
            },
            SyncTime::from_unix_millis(MARK - 30_000),
        )
        .await
        .unwrap();
    for operation in ["earliest_waiting", "sent_without_answer"] {
        plan.fail_once(operation);
    }

    let mut harness = Harness::start_with_store(root, clock_at(MARK - 20_000), store);
    let mut central = harness.connected().await;
    advance_to_next_mark(&harness);
    let first = batch_now(&harness, &mut central).await;
    advance_to_next_mark(&harness);
    let second = batch_now(&harness, &mut central).await;

    let sent = first
        .or(second)
        .expect("the waiting write is flushed after the restore failure");
    assert_eq!(sent[0].payload().write_request_id().unwrap().as_str(), id);
    harness.stop().await;
}

#[tokio::test]
async fn a_failed_send_transition_does_not_stop_flushing_on_a_healthy_socket() {
    let root = tempfile::tempdir().unwrap();
    let (store, plan) = faulty_store(&root).await;
    let mut harness = Harness::start_with_store(root, clock_at(MARK - 20_000), store);
    let mut central = harness.connected().await;
    let id = uuid_v7(31);
    harness.calls.submit(caller(), thread_post(&id)).await;
    plan.fail_once("claim_next_batch");

    advance_to_next_mark(&harness);
    let at_failed_mark = batch_now(&harness, &mut central).await;
    advance_to_next_mark(&harness);
    let next_mark = batch_now(&harness, &mut central).await;

    assert!(
        at_failed_mark.is_none(),
        "nothing went out when the transition failed"
    );
    let sent = next_mark.expect("the next mark retries from the database");
    assert_eq!(sent[0].payload().write_request_id().unwrap().as_str(), id);
    assert!(matches!(
        harness.calls.link_status().await,
        Some(central_link_client::LinkStatus::Connected { .. })
    ));
    harness.stop().await;
}

#[tokio::test]
async fn a_failed_return_or_answer_write_is_reconciled_and_the_write_answered() {
    for failing in ["return_unanswered", "record_outcome"] {
        let root = tempfile::tempdir().unwrap();
        let (store, plan) = faulty_store(&root).await;
        let mut harness = Harness::start_with_store(root, clock_at(MARK - 20_000), store);
        let mut central = harness.connected().await;
        let id = uuid_v7(32);
        harness.calls.submit(caller(), thread_post(&id)).await;
        advance_to_next_mark(&harness);
        let first = batch_now(&harness, &mut central)
            .await
            .expect("first batch");
        plan.fail_once(failing);
        if failing == "record_outcome" {
            answer_all(&central, &first);
            // The driver reads the answers before time moves on.
            for _ in 0..3 {
                let _ = harness.calls.link_status().await;
            }
        } else {
            // The answer is lost: the link drops before it arrives.
            drop(central);
            // Let the driver see the close before the backoff clock moves.
            for _ in 0..3 {
                let _ = harness.calls.link_status().await;
            }
            harness.clock.advance(Duration::from_millis(1_000)).unwrap();
            central = harness.connected().await;
        }

        let mut resent = None;
        for _ in 0..3 {
            advance_to_next_mark(&harness);
            if let Some(batch) = batch_now(&harness, &mut central).await {
                resent = Some(batch);
                break;
            }
        }
        let resent = resent.unwrap_or_else(|| panic!("{failing}: the write is resent"));
        answer_all(&central, &resent);
        for _ in 0..10 {
            let _ = harness.calls.link_status().await;
        }
        let status = harness
            .calls
            .request_status(caller(), status_query(&id))
            .await;

        assert_eq!(resent[0].payload().write_request_id().unwrap().as_str(), id);
        assert_eq!(status, RequestStatus::Answered(reply()), "{failing}");
        assert!(plan.pending().is_empty(), "{failing}: the fault fired");
        harness.stop().await;
        let _ = MemberOutcome::Answered(reply());
    }
}
