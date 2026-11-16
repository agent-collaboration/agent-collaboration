//! V6 on workerd: an unpublishable decision rolls the request batch back and
//! fails the real handler, the upper range has no mark, a crash mid fan-out
//! redoes nothing, and a dropped or never-published alarm is re-published by
//! the next request's own event.
use super::super::{
    Statement,
    support::*,
    workerd_host::{AlarmReceipt, AlarmReceiptResult},
};
use super::engine_event_support::*;
use super::engine_probe_support::*;
use central_storage::{AlarmPublicationFailure, StoreFailure};
use collaboration_central_core::{
    PausePoint,
    engine_probe::{EngineReceipt, EngineRequestOutcome, ProbeEvent, ProbeState},
};

/// A time with no later five-minute mark inside the portable range, yet far
/// enough below it that a due batch's look-ahead horizon is still supported.
const NO_LATER_MARK_UNIX_MILLIS: i64 = 9_223_372_000_000;

fn unpublishable() -> StoreFailure {
    StoreFailure::AlarmUnpublishable {
        reason: AlarmPublicationFailure::NoLaterGridMark,
    }
}
fn no_later_mark() -> Time {
    chrono::DateTime::from_timestamp_millis(NO_LATER_MARK_UNIX_MILLIS).unwrap()
}
fn fired() -> ProbeAnswer {
    EngineRequestOutcome::Answer {
        receipt: EngineReceipt::Applied {
            state: ProbeState::Fired,
        },
    }
}
/// The fixture credential expires in 2030; the upper range needs a live one.
async fn credential_through_the_upper_range(runtime: &Runtime) {
    raw(
        runtime,
        "UPDATE machine_credentials SET expires_at=?",
        vec![text("2263-01-01T00:00:00Z")],
    )
    .await;
}
async fn last_receipt(runtime: &Runtime) -> AlarmReceipt {
    let receipt: Option<AlarmReceipt> = runtime.control("raw-alarm-status", &()).await.unwrap();
    receipt.unwrap()
}

#[tokio::test]
async fn an_unpublishable_request_batch_rolls_back_inside_its_transaction() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let (overdue, overdue_group) = (probe_id(30000), probe_id(30001));
    seed_probe(
        &runtime,
        overdue,
        overdue_group,
        Some(at(start, 5, 0, 0)),
        "overdue",
    )
    .await;
    credential_through_the_upper_range(&runtime).await;
    set_now(&fixture, no_later_mark());

    let result = send_message(&fixture, 30002).await;

    assert_eq!(
        result,
        Err(unpublishable()),
        "the request's own batch could not decide its alarm"
    );
    assert_eq!(
        count(
            &runtime,
            "SELECT COUNT(*) FROM messages WHERE message_kind='direct'",
            vec![]
        )
        .await,
        0,
        "the message row rolled back with the publication"
    );
    assert_eq!(receipts(&runtime, 30002).await, 0, "no receipt either");
    fixture.host.close().await;
}

#[tokio::test]
async fn with_no_later_mark_the_cold_handler_fails_and_fires_nothing() {
    let mut fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let (overdue, overdue_group) = (probe_id(30100), probe_id(30101));
    seed_probe(
        &runtime,
        overdue,
        overdue_group,
        Some(at(start, 5, 0, 0)),
        "overdue",
    )
    .await;
    set_now(&fixture, no_later_mark());
    fixture.reopen().await;
    let cold = fixture.host.cold_runtime(&fixture.caller.organization_id);
    let before = callbacks(&cold).await;

    let handled = cold.cold_alarm().await;

    assert_eq!(
        handled,
        Err(unpublishable()),
        "the handler returns the error"
    );
    let receipt = last_receipt(&cold).await;
    assert!(receipt.initialized_cold);
    assert!(matches!(
        receipt.result,
        AlarmReceiptResult::Error {
            failure: Some(StoreFailure::AlarmUnpublishable {
                reason: AlarmPublicationFailure::NoLaterGridMark
            }),
            ..
        }
    ));
    assert!(callbacks(&cold).await > before, "the real handler ran");
    // Reads go through a fresh runtime, whose startup publication fails the
    // same way; only the restored time lets the store be read back.
    set_now(&fixture, at(start, 1, 0, 0));
    let readable = fixture.host.central(&fixture.caller.organization_id).await;
    assert_eq!(
        probe_facts(&readable, overdue, overdue_group).await,
        vec![text("waiting"), integer(0), integer(0)],
        "no unit ran in the failed handler"
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn a_crash_mid_fan_out_redoes_no_done_item_and_fires_the_rest_once() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let due = at(start, 5, 0, 0);
    let (done, done_group) = (probe_id(30200), probe_id(30201));
    let (committed, committed_group) = (probe_id(30202), probe_id(30203));
    let (loaded, loaded_group) = (probe_id(30204), probe_id(30205));
    seed_probe(&runtime, done, done_group, Some(due), "done").await;
    seed_probe(&runtime, committed, committed_group, Some(due), "committed").await;
    seed_probe(&runtime, loaded, loaded_group, Some(due), "loaded").await;
    let mut after_commit = runtime
        .pause_points()
        .hold_engine(PausePoint::AfterEngineCommit, &committed.to_string())
        .await;
    let mut after_load = runtime
        .pause_points()
        .hold_engine(PausePoint::AfterEngineLoad, &loaded.to_string())
        .await;
    set_now(&fixture, due);
    let batch = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });
    after_commit.until_held().await;
    after_load.until_held().await;
    while probe_facts(&runtime, done, done_group).await[0] != text("fired") {
        tokio::task::yield_now().await;
    }

    // The existing crash cut: the object resets with both units in flight.
    let reset: Result<bool, _> = runtime.control("crash", &()).await;
    assert!(reset.is_err());
    batch.abort();
    let _ = batch.await;
    drop((after_commit, after_load));

    let cold = fixture.host.cold_runtime(&fixture.caller.organization_id);
    assert_eq!(
        probe_facts(&cold, committed, committed_group).await,
        vec![text("fired"), integer(1), integer(0)],
        "the unit held after its commit kept its commit"
    );
    cold.cold_alarm().await.unwrap();
    for (item, group) in [
        (done, done_group),
        (committed, committed_group),
        (loaded, loaded_group),
    ] {
        assert_eq!(
            probe_facts(&cold, item, group).await,
            vec![text("fired"), integer(1), integer(0)],
            "every item fired exactly once"
        );
    }
    assert_eq!(alarm_ms(&cold).await, None);
    fixture.host.close().await;
}

#[tokio::test]
async fn a_dropped_alarm_is_republished_by_the_next_requests_own_event() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let (pending, pending_group) = (probe_id(30300), probe_id(30301));
    let (traffic, traffic_group) = (probe_id(30302), probe_id(30303));
    seed_probe(
        &runtime,
        pending,
        pending_group,
        Some(at(start, 20, 0, 0)),
        "pending",
    )
    .await;
    seed_probe(&runtime, traffic, traffic_group, None, "traffic").await;
    due_batch_at(&fixture, &runtime, at(start, 1, 0, 0)).await;
    assert_eq!(alarm_ms(&runtime).await, ms(at(start, 20, 0, 0)));
    assert!(
        runtime
            .control::<bool>("raw-drop-alarm", &())
            .await
            .unwrap()
    );
    assert_eq!(alarm_ms(&runtime).await, None, "the physical alarm is gone");
    set_now(&fixture, at(start, 1, 10, 0));

    let answer = runtime
        .request_probe(
            &fixture.caller,
            probe_request(30304, traffic, ProbeEvent::Run),
        )
        .await
        .unwrap();

    assert_eq!(answer, fired());
    assert_eq!(
        alarm_ms(&runtime).await,
        ms(at(start, 20, 0, 0)),
        "the request's own event re-published the alarm"
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn a_failed_request_publication_without_an_old_alarm_is_republished_by_the_next_request() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let (pending, pending_group) = (probe_id(30400), probe_id(30401));
    let (first, second, traffic_group) = (probe_id(30402), probe_id(30403), probe_id(30404));
    seed_probe(
        &runtime,
        pending,
        pending_group,
        Some(at(start, 20, 0, 0)),
        "pending",
    )
    .await;
    seed_probe(&runtime, first, traffic_group, None, "first").await;
    seed_probe(&runtime, second, traffic_group, None, "second").await;
    assert_eq!(alarm_ms(&runtime).await, None, "seeding published nothing");
    assert!(
        runtime
            .control::<bool>("fail-alarm-batch", &StoreFailure::Unavailable)
            .await
            .unwrap()
    );
    set_now(&fixture, at(start, 1, 0, 0));

    let answer = runtime
        .request_probe(
            &fixture.caller,
            probe_request(30405, first, ProbeEvent::Run),
        )
        .await
        .unwrap();

    assert_eq!(answer, fired(), "the request returns its committed answer");
    assert_eq!(receipts(&runtime, 30405).await, 1);
    assert_eq!(
        alarm_ms(&runtime).await,
        None,
        "timed work is pending with no physical alarm (alarmMissing)"
    );
    assert_eq!(
        probe_facts(&runtime, pending, pending_group).await,
        vec![text("waiting"), integer(0), integer(0)]
    );
    set_now(&fixture, at(start, 1, 10, 0));
    let next = runtime
        .request_probe(
            &fixture.caller,
            probe_request(30406, second, ProbeEvent::Run),
        )
        .await
        .unwrap();
    assert_eq!(next, fired());
    assert_eq!(
        alarm_ms(&runtime).await,
        ms(at(start, 20, 0, 0)),
        "the next request re-published the alarm"
    );
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT COUNT(*) FROM engine_probe_items WHERE group_id=? AND state='fired'",
            args: vec![text(traffic_group)],
        })
        .await
        .unwrap();
    assert_eq!(rows.rows, vec![vec![integer(2)]]);
    fixture.host.close().await;
}
