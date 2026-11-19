//! Pre-admission cancel (Lead A, from PR 4's review): a caller that goes
//! away before the driver admits its write leaves no row and no frame; once
//! the insert commits, nothing is undone. The test holds the store lock to
//! keep the driver busy with an earlier command while the caller goes away.
use super::super::driver_test_support::*;
use central_link_client::{LinkCalls, RequestStatus};
use central_link_protocol::{DataFrame, LinkFrame};
use std::time::Duration;

async fn let_tasks_run() {
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn a_caller_gone_before_admission_leaves_no_row_and_no_frame() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let mut central = harness.connected().await;
    let kept = uuid_v7(60);
    let dropped = uuid_v7(61);

    let held = harness.store.lock().await;
    let calls = harness.calls.clone();
    let first = tokio::spawn({
        let calls = calls.clone();
        let kept = kept.clone();
        async move { calls.submit(caller(), thread_post(&kept)).await }
    });
    let_tasks_run().await;
    let gone_write = tokio::spawn({
        let calls = calls.clone();
        let dropped = dropped.clone();
        async move { calls.submit(caller(), thread_post(&dropped)).await }
    });
    let gone_read = tokio::spawn({
        let calls = calls.clone();
        async move { calls.submit(caller(), list_areas()).await }
    });
    let_tasks_run().await;
    gone_write.abort();
    gone_read.abort();
    let_tasks_run().await;
    drop(held);
    first.await.unwrap();
    let unresolved = harness
        .calls
        .request_status(caller(), status_query(&dropped))
        .await;
    let dropped_status = harness.local_status(&dropped).await;
    harness
        .clock
        .advance(Duration::from_millis(20_000))
        .unwrap();
    let frame = central.frame().await;

    assert_eq!(
        dropped_status,
        RequestStatus::NotFound,
        "no row was written"
    );
    assert_eq!(unresolved, RequestStatus::StatusUnavailable);
    let LinkFrame::Data(DataFrame::Batch { requests }) = frame else {
        panic!("the first frame is the minute batch, not the dropped read: {frame:?}");
    };
    let sent: Vec<_> = requests
        .requests()
        .iter()
        .map(|member| {
            member
                .payload()
                .write_request_id()
                .unwrap()
                .as_str()
                .to_owned()
        })
        .collect();
    assert_eq!(
        sent,
        vec![kept],
        "only the write whose caller stayed is sent"
    );
    harness.stop().await;
}

#[tokio::test]
async fn a_caller_gone_after_the_insert_still_has_its_write_flushed_and_answered() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let mut central = harness.connected().await;
    let id = uuid_v7(62);

    let submit = tokio::spawn({
        let calls = harness.calls.clone();
        let id = id.clone();
        async move { calls.submit(caller(), thread_post(&id)).await }
    });
    let mut stored = false;
    for _ in 0..64 {
        let status = harness
            .store
            .lock()
            .await
            .request_status(&caller(), &id.parse().unwrap())
            .await
            .unwrap();
        if status != RequestStatus::NotFound {
            stored = true;
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(stored, "the insert committed");
    submit.abort();
    harness
        .clock
        .advance(Duration::from_millis(20_000))
        .unwrap();
    let LinkFrame::Data(DataFrame::Batch { requests }) = central.frame().await else {
        panic!("the write is flushed");
    };
    for member in requests.requests() {
        central.send(LinkFrame::Data(DataFrame::Reply {
            correlation_id: member.correlation_id().clone(),
            result: Box::new(reply()),
        }));
    }
    let mut status = RequestStatus::Sent;
    for _ in 0..64 {
        status = harness
            .calls
            .request_status(caller(), status_query(&id))
            .await;
        if status != RequestStatus::Sent {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert_eq!(requests.requests().len(), 1);
    assert_eq!(
        status,
        RequestStatus::Answered(reply()),
        "nothing is undone"
    );
    harness.stop().await;
}
