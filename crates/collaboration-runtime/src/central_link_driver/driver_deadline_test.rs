//! Step 9 B (Lead A, 2026-11-18): a write must never be replayed past its
//! deadline, 15 minutes for every outbox write (owner, 2026-11-18). Central commits a write,
//! its answer is lost, the Router stops, and it starts again after the
//! deadline: the write ends with its agent told, and central never sees it a
//! second time.
use super::super::super::driver_test_support::*;
use super::super::super::*;
use central_link_client::{LinkCalls, OutboxCancellation, QueuedWrite, RequestStatus};
use central_link_protocol::{
    DataFrame, FrameDirection, LinkFrame, LinkRequest, RequestPayload, decode_text,
};
use runtime_primitives::Clock;
use std::time::Duration;

/// Every batch central has received since the last look; heartbeats answered.
async fn batches_now(harness: &Harness, central: &mut CentralEnd) -> Vec<Vec<LinkRequest>> {
    for _ in 0..3 {
        let _ = harness.calls.link_status().await;
    }
    let mut batches = Vec::new();
    while let Ok(text) = central.from_router.try_recv() {
        match decode_text(&text, FrameDirection::ToCentral).unwrap() {
            LinkFrame::Ping => central.send(LinkFrame::Pong),
            LinkFrame::Data(DataFrame::Batch { requests }) => {
                batches.push(requests.requests().to_vec());
            }
            other => panic!("unexpected frame: {other:?}"),
        }
    }
    batches
}

fn advance_to_next_mark(harness: &Harness) {
    let now = harness.clock.now().unix_millis();
    let next = (now.div_euclid(60_000) + 1) * 60_000;
    harness
        .clock
        .advance(Duration::from_millis(u64::try_from(next - now).unwrap()))
        .unwrap();
}

async fn reopened(database: &std::path::Path) -> Arc<Mutex<dyn OutboxStorage>> {
    Arc::new(Mutex::new(open_outbox(database).await.unwrap()))
}

/// Runs the scenario for one write and returns how it ended and how many
/// times central received it.
async fn committed_then_lost_then_restarted_after(
    payload: QueuedWrite,
    id: &str,
    restart_after_ms: i64,
) -> (RequestStatus, Vec<central_link_client::EndedWrite>, usize) {
    let database_root = tempfile::tempdir().unwrap();
    let database = database_root.path().join("machine_local.sqlite");
    let mut received = 0;

    let mut first = Harness::start_with_store(
        tempfile::tempdir().unwrap(),
        clock_at(MARK - 20_000),
        reopened(&database).await,
    );
    let mut central = first.connected().await;
    match payload {
        QueuedWrite::Workspace(request) => {
            first
                .calls
                .submit(caller(), RequestPayload::Workspace(request))
                .await
        }
        QueuedWrite::SendMessage(request) => first.calls.submit_message(caller(), request).await,
    };
    advance_to_next_mark(&first);
    for batch in batches_now(&first, &mut central).await {
        received += batch.len();
    }
    assert_eq!(received, 1, "central received and committed the write");
    // The answer is lost: the Router stops before any reply arrives.
    first.stop().await;
    drop(central);

    let restart_at = MARK - 20_000 + restart_after_ms;
    let mut second = Harness::start_with_store(
        tempfile::tempdir().unwrap(),
        clock_at(restart_at),
        reopened(&database).await,
    );
    let mut central = second.connected().await;
    for _ in 0..3 {
        advance_to_next_mark(&second);
        for batch in batches_now(&second, &mut central).await {
            received += batch.len();
            // Central answers a replay from its request log.
            for member in &batch {
                central.send(LinkFrame::Data(DataFrame::Reply {
                    correlation_id: member.correlation_id().clone(),
                    result: Box::new(reply()),
                }));
            }
            for _ in 0..3 {
                let _ = second.calls.link_status().await;
            }
        }
    }
    for _ in 0..5 {
        let _ = second.calls.link_status().await;
    }
    let status = second
        .calls
        .request_status(caller(), id.parse().unwrap())
        .await;
    let ended = second.outcomes.1.lock().unwrap().clone();
    second.stop().await;
    (status, ended, received)
}

#[tokio::test]
async fn a_committed_post_whose_answer_was_lost_is_never_resent_after_15_minutes() {
    let id = uuid_v7(40);
    let (status, ended, received) =
        committed_then_lost_then_restarted_after(queued_thread_post(&id), &id, 16 * 60 * 1000)
            .await;

    assert_eq!(received, 1, "no second send past the deadline");
    assert_eq!(
        status,
        RequestStatus::Cancelled {
            reason: OutboxCancellation::NotConfirmedWithin15Minutes
        }
    );
    assert_eq!(
        ended
            .iter()
            .map(|write| (write.request_id.as_str().to_owned(), write.reason))
            .collect::<Vec<_>>(),
        vec![(id, OutboxCancellation::NotConfirmedWithin15Minutes)],
        "the agent is told once"
    );
}

#[tokio::test]
async fn a_direct_message_whose_answer_was_lost_ends_unconfirmed_after_15_minutes() {
    let id = uuid_v7(41);
    let (status, ended, received) = committed_then_lost_then_restarted_after(
        QueuedWrite::SendMessage(direct_message(&id)),
        &id,
        16 * 60 * 1000,
    )
    .await;

    assert_eq!(received, 1, "no second send past the deadline");
    assert_eq!(
        status,
        RequestStatus::Cancelled {
            reason: OutboxCancellation::NotConfirmedWithin15Minutes
        }
    );
    assert_eq!(ended.len(), 1);
    assert_eq!(
        ended[0].reason,
        OutboxCancellation::NotConfirmedWithin15Minutes
    );
}

#[tokio::test]
async fn a_write_restarted_before_its_deadline_is_resent_under_the_same_id() {
    let id = uuid_v7(42);
    let (status, ended, received) =
        committed_then_lost_then_restarted_after(queued_thread_post(&id), &id, 10 * 60 * 1000)
            .await;

    assert_eq!(received, 2, "inside the window the replay goes out again");
    assert_eq!(status, RequestStatus::Answered(reply()));
    assert!(ended.is_empty());
}

/// A post stored while central was unreachable, never sent, and the Router
/// restarted after 15 minutes: it ends NotSent, its agent told, and central
/// never sees it.
#[tokio::test]
async fn a_post_that_never_left_the_mac_ends_not_sent_after_15_minutes() {
    let database_root = tempfile::tempdir().unwrap();
    let database = database_root.path().join("machine_local.sqlite");
    let id = uuid_v7(43);

    let first = Harness::start_with_store(
        tempfile::tempdir().unwrap(),
        clock_at(MARK - 20_000),
        reopened(&database).await,
    );
    // Central never answers the connection: the write only waits.
    first.calls.submit(caller(), thread_post(&id)).await;
    first.stop().await;

    let mut second = Harness::start_with_store(
        tempfile::tempdir().unwrap(),
        clock_at(MARK - 20_000 + 16 * 60 * 1000),
        reopened(&database).await,
    );
    let mut central = second.connected().await;
    let mut received = 0;
    for _ in 0..3 {
        advance_to_next_mark(&second);
        for batch in batches_now(&second, &mut central).await {
            received += batch.len();
        }
    }
    let status = second
        .calls
        .request_status(caller(), id.parse().unwrap())
        .await;
    let ended = second.outcomes.1.lock().unwrap().clone();
    second.stop().await;

    assert_eq!(received, 0, "an overdue write is never sent");
    assert_eq!(
        status,
        RequestStatus::Cancelled {
            reason: OutboxCancellation::NotSentWithin15Minutes
        }
    );
    assert_eq!(
        ended
            .iter()
            .map(|write| (write.request_id.as_str().to_owned(), write.reason))
            .collect::<Vec<_>>(),
        vec![(id, OutboxCancellation::NotSentWithin15Minutes)]
    );
}

#[tokio::test]
async fn an_offline_router_ends_waiting_writes_at_the_exact_deadline() {
    let database_root = tempfile::tempdir().unwrap();
    let database = database_root.path().join("machine_local.sqlite");
    // Use a creation time between minute marks, so the minute flush could
    // not satisfy this deadline even if it were running.
    let harness = Harness::start_with_store(
        tempfile::tempdir().unwrap(),
        clock_at(MARK - 18_765),
        reopened(&database).await,
    );
    let post = uuid_v7(44);
    let message = uuid_v7(45);
    harness.calls.submit(caller(), thread_post(&post)).await;
    harness
        .calls
        .submit_message(caller(), direct_message(&message))
        .await;

    harness
        .clock
        .advance(Duration::from_millis(15 * 60 * 1000 - 1))
        .unwrap();
    let _ = harness.calls.link_status().await;
    assert!(harness.outcomes.1.lock().unwrap().is_empty());

    harness.clock.advance(Duration::from_millis(1)).unwrap();
    let _ = harness.calls.link_status().await;
    let mut ended = harness.outcomes.1.lock().unwrap().clone();
    ended.sort_by_key(|outcome| outcome.request_id.clone());
    assert_eq!(
        ended
            .iter()
            .map(|outcome| (outcome.request_id.as_str(), outcome.reason))
            .collect::<Vec<_>>(),
        [
            (post.as_str(), OutboxCancellation::NotSentWithin15Minutes),
            (message.as_str(), OutboxCancellation::NotSentWithin15Minutes),
        ],
    );
    harness.stop().await;
}
