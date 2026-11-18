//! The production driver against a scripted central over an in-memory socket,
//! on the injected ManualClock. It proves the driver's own behaviour (minute
//! flush, batch frame, outcomes, link loss, restart); a real central socket is
//! 2d's Turmoil journey.
use super::super::driver_test_support::*;
use super::super::*;
use crate::outbox_store::{MacOutbox, NewOutboxItem};
use central_link_client::{LinkCalls, PendingWriteState, RequestStatus, Submission, SubmitRefusal};
use central_link_protocol::{DataFrame, LinkFrame};
use std::time::Duration;

#[tokio::test]
async fn a_post_flushes_on_its_mark_as_one_batch_and_its_answer_is_reported() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let mut central = harness.connected().await;
    let id = uuid_v7(1);

    let submission = harness.calls.submit(caller(), thread_post(&id)).await;
    harness
        .clock
        .advance(Duration::from_millis(20_000))
        .unwrap();
    let LinkFrame::Data(DataFrame::Batch { requests }) = central.frame().await else {
        panic!("the flush sends one batch frame");
    };
    let [member] = requests.requests() else {
        panic!("one member");
    };
    central.send(LinkFrame::Data(DataFrame::Reply {
        correlation_id: member.correlation_id().clone(),
        result: Box::new(reply()),
    }));
    let status = loop {
        let status = harness
            .calls
            .request_status(caller(), id.parse().unwrap())
            .await;
        if status != RequestStatus::Sent {
            break status;
        }
        tokio::task::yield_now().await;
    };

    assert!(matches!(submission, Submission::InOutbox(ref receipt)
        if receipt.flush_at == SyncTime::from_unix_millis(MARK)));
    assert_eq!(status, RequestStatus::Answered(reply()));
    let outcomes = harness.outcomes.0.lock().unwrap().clone();
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].complete);
    assert_eq!(outcomes[0].outcomes[0].request_id.as_str(), id);
    harness.stop().await;
}

#[tokio::test]
async fn a_lost_link_returns_the_batch_as_sent_answer_lost_and_resends_the_same_ids() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let central = harness.connected().await;
    let id = uuid_v7(2);
    harness.calls.submit(caller(), thread_post(&id)).await;
    let mut central = central;

    harness
        .clock
        .advance(Duration::from_millis(20_000))
        .unwrap();
    let LinkFrame::Data(DataFrame::Batch { requests }) = central.frame().await else {
        panic!("first batch");
    };
    let first = requests.requests()[0].clone();
    drop(central);
    let pending = harness.calls.pending_for(caller()).await.unwrap();
    // The client reconnects after its 1 s backoff on the injected clock.
    harness.clock.advance(Duration::from_millis(1_000)).unwrap();
    let mut central = harness.connected().await;
    harness
        .clock
        .advance(Duration::from_millis(60_000))
        .unwrap();
    let LinkFrame::Data(DataFrame::Batch { requests }) = central.frame().await else {
        panic!("resent batch");
    };

    assert_eq!(pending.len(), 1);
    assert!(matches!(
        pending[0].state,
        PendingWriteState::SentAnswerLost { ref body } if body.as_str() == "hi"
    ));
    let outcomes = harness.outcomes.0.lock().unwrap().clone();
    assert!(matches!(outcomes.as_slice(), [report] if !report.complete));
    let resent = &requests.requests()[0];
    assert_eq!(
        resent.payload().write_request_id(),
        first.payload().write_request_id()
    );
    assert_ne!(resent.correlation_id(), first.correlation_id());
    harness.stop().await;
}

#[tokio::test]
async fn immediate_calls_are_answered_on_the_link_and_refused_when_it_is_down() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let down = harness.calls.submit(caller(), list_areas()).await;
    let mut central = harness.connected().await;

    let answered = tokio::spawn({
        let calls = harness.calls.clone();
        async move { calls.submit(caller(), list_areas()).await }
    });
    let LinkFrame::Data(DataFrame::Request { request }) = central.frame().await else {
        panic!("an immediate request frame");
    };
    central.send(LinkFrame::Data(DataFrame::Reply {
        correlation_id: request.correlation_id().clone(),
        result: Box::new(reply()),
    }));

    assert!(matches!(
        down,
        Submission::Refused(SubmitRefusal::CentralUnavailable)
    ));
    assert!(matches!(answered.await.unwrap(), Submission::Answered(_)));
    harness.stop().await;
}

#[tokio::test]
async fn a_restart_resends_writes_that_were_sent_without_an_answer() {
    let root = tempfile::tempdir().unwrap();
    let clock = clock_at(MARK - 20_000);
    let store = Arc::new(Mutex::new(
        MachineLocalStore::open(&root.path().join("machine_local.sqlite"))
            .await
            .unwrap(),
    ));
    let id = uuid_v7(3);
    {
        let mut locked = store.lock().await;
        let mut outbox = MacOutbox::new(&mut locked);
        let item = NewOutboxItem {
            request: id.parse().unwrap(),
            caller: caller(),
            payload: thread_post(&id),
            flush_at: SyncTime::from_unix_millis(MARK),
        };
        outbox
            .insert(&item, SyncTime::from_unix_millis(MARK - 30_000))
            .await
            .unwrap();
        outbox
            .mark_sent(
                std::slice::from_ref(&item.request),
                SyncTime::from_unix_millis(MARK - 25_000),
            )
            .await
            .unwrap();
    }

    let mut harness = Harness::start_on(root, clock, store);
    let mut central = harness.connected().await;
    harness
        .clock
        .advance(Duration::from_millis(20_000))
        .unwrap();
    let LinkFrame::Data(DataFrame::Batch { requests }) = central.frame().await else {
        panic!("the restored write is resent on its mark");
    };

    assert_eq!(
        requests.requests()[0]
            .payload()
            .write_request_id()
            .map(|id| id.as_str().to_owned()),
        Some(id)
    );
    let _ = &harness.store;
    harness.stop().await;
}

#[tokio::test]
async fn a_machine_revoked_answer_returns_the_write_unsent_and_the_next_mark_resends_it() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let mut central = harness.connected().await;
    let id = uuid_v7(4);
    harness.calls.submit(caller(), thread_post(&id)).await;
    harness
        .clock
        .advance(Duration::from_millis(20_000))
        .unwrap();
    let LinkFrame::Data(DataFrame::Batch { requests }) = central.frame().await else {
        panic!("first batch");
    };
    let first = requests.requests()[0].clone();

    // Link admission refused it: central did not apply the write.
    central.send(LinkFrame::Data(DataFrame::Reply {
        correlation_id: first.correlation_id().clone(),
        result: Box::new(machine_revoked()),
    }));
    let pending = harness.calls.pending_for(caller()).await.unwrap();
    let status = harness
        .calls
        .request_status(caller(), id.parse().unwrap())
        .await;
    assert!(matches!(
        &pending[..],
        [write] if matches!(&write.state, PendingWriteState::Unsent { body } if body.as_str() == "hi")
    ));
    assert!(matches!(status, RequestStatus::InOutbox { .. }));
    let outcomes = harness.outcomes.0.lock().unwrap().clone();
    assert!(
        matches!(outcomes.as_slice(), [report] if !report.complete && report.outcomes.is_empty()),
        "no outcome for a write central did not apply: {outcomes:?}"
    );
    harness
        .clock
        .advance(Duration::from_millis(60_000))
        .unwrap();
    let LinkFrame::Data(DataFrame::Batch { requests }) = central.frame().await else {
        panic!("resent batch");
    };

    assert_eq!(
        requests.requests()[0].payload().write_request_id(),
        first.payload().write_request_id()
    );
    harness.stop().await;
}

#[tokio::test]
async fn a_flush_tick_prunes_writes_that_ended_60_days_ago() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(
        MachineLocalStore::open(&root.path().join("machine_local.sqlite"))
            .await
            .unwrap(),
    ));
    let sixty_days_ms: i64 = 60 * 24 * 60 * 60 * 1000;
    let old = uuid_v7(5);
    {
        let mut locked = store.lock().await;
        let mut outbox = MacOutbox::new(&mut locked);
        let item = NewOutboxItem {
            request: old.parse().unwrap(),
            caller: caller(),
            payload: thread_post(&old),
            flush_at: SyncTime::from_unix_millis(MARK - sixty_days_ms),
        };
        let ended_at = SyncTime::from_unix_millis(MARK - sixty_days_ms - 1_000);
        outbox.insert(&item, ended_at).await.unwrap();
        outbox
            .mark_sent(std::slice::from_ref(&item.request), ended_at)
            .await
            .unwrap();
        outbox
            .record_outcome(
                &item.request,
                &central_link_client::MemberOutcome::Answered(reply()),
                ended_at,
            )
            .await
            .unwrap();
    }
    let mut harness = Harness::start_on(root, clock_at(MARK - 20_000), store.clone());
    let mut central = harness.connected().await;
    let fresh = uuid_v7(6);
    harness.calls.submit(caller(), thread_post(&fresh)).await;
    let before = harness
        .calls
        .request_status(caller(), old.parse().unwrap())
        .await;

    harness
        .clock
        .advance(Duration::from_millis(20_000))
        .unwrap();
    let LinkFrame::Data(DataFrame::Batch { .. }) = central.frame().await else {
        panic!("the flush tick sends its batch");
    };
    let after = harness
        .calls
        .request_status(caller(), old.parse().unwrap())
        .await;

    assert!(matches!(before, RequestStatus::Answered(_)));
    assert_eq!(after, RequestStatus::NotFound, "pruned at the tick");
    harness.stop().await;
}
