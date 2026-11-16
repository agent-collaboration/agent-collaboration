//! V6 on the native host: a post-commit publication failure is reported and keeps
//! the previous alarm, the upper range has no mark and arms nothing, a restart
//! mid fan-out redoes nothing, and a dropped alarm is re-published by traffic.
use super::super::{Statement, support::*};
use super::engine_event_support::*;
use super::engine_probe_support::*;
use central_storage::{
    AlarmPublicationFailure, AlarmUpdate, BatchOutcome, StatementBatch, StoreFailure,
};
use collaboration_central_core::{
    AlarmNotPublished, MAX_ALARM_UNIX_MILLIS, PausePoint,
    engine_probe::{EngineReceipt, EngineRequestOutcome, ProbeEvent, ProbeState},
};
use router_clock::RuntimeClock;

/// A time with no later five-minute mark inside the portable range, yet far
/// enough below it that a due batch's look-ahead horizon is still supported.
const NO_LATER_MARK_UNIX_MILLIS: i64 = 9_223_372_000_000;

fn unpublishable() -> AlarmNotPublished {
    AlarmNotPublished {
        failure: StoreFailure::AlarmUnpublishable {
            reason: AlarmPublicationFailure::NoLaterGridMark,
        },
    }
}
fn from_ms(unix_millis: i64) -> Time {
    chrono::DateTime::from_timestamp_millis(unix_millis).unwrap()
}
fn fired() -> ProbeAnswer {
    EngineRequestOutcome::Answer {
        receipt: EngineReceipt::Applied {
            state: ProbeState::Fired,
        },
    }
}

#[tokio::test]
async fn a_post_commit_publication_failure_keeps_committed_rows_and_the_previous_alarm() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    let (pending, pending_group) = (probe_id(30000), probe_id(30001));
    let (traffic, traffic_group) = (probe_id(30002), probe_id(30003));
    seed_probe(
        &runtime,
        pending,
        pending_group,
        Some(at(start, 10, 0, 0)),
        "pending",
    )
    .await;
    seed_probe(&runtime, traffic, traffic_group, None, "traffic").await;
    due_batch_at(&fixture, &runtime, at(start, 1, 0, 0)).await;
    let previous = alarm_ms(&runtime);
    assert_eq!(previous, ms(at(start, 10, 0, 0)));
    assert!(
        runtime
            .statement_host()
            .take_alarm_publication_failures()
            .is_empty()
    );
    // The fixture credential expires in 2030; the upper edge needs a live one.
    raw(
        &runtime,
        "UPDATE machine_credentials SET expires_at=?",
        vec![text("2263-01-01T00:00:00Z")],
    )
    .await;
    // A due batch claimed here has no representable 30 s floor, so the
    // decision fails only after the database committed.
    let edge = MAX_ALARM_UNIX_MILLIS - 10_000;
    fixture.clock.set_utc(from_ms(edge));

    let answer = runtime
        .request_probe(
            &fixture.caller,
            probe_request(30004, traffic, ProbeEvent::Run),
        )
        .await
        .unwrap();

    assert_eq!(answer, fired(), "the committed answer is returned");
    assert_eq!(
        probe_facts(&runtime, traffic, traffic_group).await,
        vec![text("fired"), integer(1), integer(0)],
        "the request's rows committed"
    );
    assert_eq!(receipts(&runtime, 30004).await, 1);
    // Pushed only for BatchOutcome::Applied { publication: Failed { .. } }.
    assert_eq!(
        runtime.statement_host().take_alarm_publication_failures(),
        vec![AlarmPublicationFailure::NoLaterGridMark]
    );
    assert_eq!(alarm_ms(&runtime), previous, "the previous alarm is kept");
    assert_eq!(
        runtime.statement_host().failed_alarm_publication_at(),
        Some(runtime_primitives::SyncTime::from_unix_millis(edge))
    );

    let completion = runtime.after_event().await;

    assert_eq!(completion, Err(unpublishable()));
    assert_eq!(
        runtime.statement_host().take_alarm_publication_failures(),
        vec![AlarmPublicationFailure::NoLaterGridMark],
        "the final pass batch applied and reported Failed"
    );
    assert_eq!(alarm_ms(&runtime), previous);
    assert_eq!(
        probe_facts(&runtime, traffic, traffic_group).await,
        vec![text("fired"), integer(1), integer(0)]
    );
    assert_eq!(
        probe_facts(&runtime, pending, pending_group).await,
        vec![text("waiting"), integer(0), integer(0)]
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn with_no_later_mark_the_alarm_is_unpublishable_and_nothing_is_armed() {
    let mut fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    let (item, group) = (probe_id(30100), probe_id(30101));
    seed_probe(&runtime, item, group, Some(at(start, 5, 0, 0)), "overdue").await;
    due_batch_at(&fixture, &runtime, at(start, 1, 0, 0)).await;
    let previous = alarm_ms(&runtime);
    assert_eq!(previous, ms(at(start, 5, 0, 0)));
    assert!(
        (NO_LATER_MARK_UNIX_MILLIS.div_euclid(300_000) + 1) * 300_000 > MAX_ALARM_UNIX_MILLIS,
        "the fixture time has no later mark"
    );
    let completed = runtime.alarm_completion_count();
    fixture.clock.set_utc(from_ms(NO_LATER_MARK_UNIX_MILLIS));

    let published = runtime.recompute_alarm().await;

    assert_eq!(published, Err(unpublishable().failure));
    assert_eq!(
        runtime.statement_host().take_alarm_publication_failures(),
        vec![AlarmPublicationFailure::NoLaterGridMark]
    );
    assert_eq!(
        runtime.statement_host().failed_alarm_publication_at(),
        Some(runtime_primitives::SyncTime::from_unix_millis(
            NO_LATER_MARK_UNIX_MILLIS
        ))
    );
    assert_eq!(alarm_ms(&runtime), previous, "no alarm is fabricated");
    // The owner reads the failed status before time moves; it has no mark to
    // retry at, so it waits for the next publication instead of a deadline.
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
    fixture
        .clock
        .advance(std::time::Duration::from_secs(3_600))
        .unwrap();
    fixture.clock.set_utc(from_ms(NO_LATER_MARK_UNIX_MILLIS));
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        runtime.alarm_completion_count(),
        completed,
        "neither the old 10:05 deadline nor a retry fired"
    );
    assert_eq!(
        probe_facts(&runtime, item, group).await,
        vec![text("waiting"), integer(0), integer(0)],
        "no event ran the overdue item"
    );
    assert!(
        runtime
            .statement_host()
            .take_alarm_publication_failures()
            .is_empty(),
        "nothing looped on the failure"
    );
    drop(runtime);

    fixture.reopen().await;

    assert_eq!(
        fixture
            .host
            .central(&fixture.caller.organization_id)
            .await
            .err(),
        Some(unpublishable().failure),
        "startup observes the same failure and starts no alarm owner"
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn a_restart_mid_fan_out_redoes_no_done_item_and_fires_the_rest_once() {
    let mut fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    let due = at(start, 5, 0, 0);
    let (done, done_group) = (probe_id(30200), probe_id(30201));
    let (committed, committed_group) = (probe_id(30202), probe_id(30203));
    let (loaded, loaded_group) = (probe_id(30204), probe_id(30205));
    seed_probe(&runtime, done, done_group, Some(due), "done").await;
    seed_probe(&runtime, committed, committed_group, Some(due), "committed").await;
    seed_probe(&runtime, loaded, loaded_group, Some(due), "loaded").await;
    let mut after_commit = runtime
        .pause_points()
        .hold_engine(PausePoint::AfterEngineCommit, &committed.to_string());
    let mut after_load = runtime
        .pause_points()
        .hold_engine(PausePoint::AfterEngineLoad, &loaded.to_string());
    fixture.clock.set_utc(due);
    let batch = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });
    after_commit.until_held().await;
    after_load.until_held().await;
    while probe_facts(&runtime, done, done_group).await[0] != text("fired") {
        tokio::task::yield_now().await;
    }

    // The crash cut: the event's future goes away with both units in flight.
    batch.abort();
    assert!(batch.await.unwrap_err().is_cancelled());
    drop((after_commit, after_load, runtime));
    fixture.reopen().await;

    let runtime = super::engine_probe_support::runtime(&fixture).await;
    assert_eq!(
        probe_facts(&runtime, committed, committed_group).await,
        vec![text("fired"), integer(1), integer(0)],
        "the unit held after its commit kept its commit"
    );
    assert_eq!(
        probe_facts(&runtime, loaded, loaded_group).await,
        vec![text("waiting"), integer(0), integer(0)],
        "the unit held after its load committed nothing"
    );
    due_batch_at(&fixture, &runtime, at(start, 5, 1, 0)).await;
    for (item, group) in [
        (done, done_group),
        (committed, committed_group),
        (loaded, loaded_group),
    ] {
        assert_eq!(
            probe_facts(&runtime, item, group).await,
            vec![text("fired"), integer(1), integer(0)],
            "every item fired exactly once"
        );
    }
    assert_eq!(alarm_ms(&runtime), None);
    fixture.host.close().await;
}

#[tokio::test]
async fn a_dropped_alarm_is_republished_by_the_next_requests_own_event() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
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
    assert_eq!(alarm_ms(&runtime), ms(at(start, 20, 0, 0)));
    // The native twin of /test/raw-drop-alarm: an empty plain SetFromQuery NULL.
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![],
                alarm: AlarmUpdate::SetFromQuery(Statement {
                    sql: "SELECT NULL",
                    args: vec![],
                }),
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    assert_eq!(alarm_ms(&runtime), None, "the physical alarm is gone");
    fixture.clock.set_utc(at(start, 1, 10, 0));

    let answer = runtime
        .request_probe(
            &fixture.caller,
            probe_request(30304, traffic, ProbeEvent::Run),
        )
        .await
        .unwrap();

    assert_eq!(answer, fired());
    assert_eq!(
        alarm_ms(&runtime),
        ms(at(start, 20, 0, 0)),
        "the request's own event re-published the alarm"
    );
    assert_eq!(
        probe_facts(&runtime, pending, pending_group).await,
        vec![text("waiting"), integer(0), integer(0)]
    );
    fixture.host.close().await;
}
