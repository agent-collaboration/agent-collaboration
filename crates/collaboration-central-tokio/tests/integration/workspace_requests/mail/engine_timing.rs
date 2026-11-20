//! V12: an event starts the due batch, its spacing, look-ahead and fallback alarm.
//! Times are named by their wall time when the fixture starts at 10:00:00.
use super::super::support::*;
use super::engine_probe_support::*;
use collaboration_central_core::engine_probe::{
    EngineReceipt, EngineRequestOutcome, ProbeEvent, ProbeState,
};
use router_clock::RuntimeClock;

type Time = chrono::DateTime<chrono::Utc>;

/// The wall time `minutes:seconds.millis` after the fixture's 10:00:00 mark.
fn at(start: Time, minutes: i64, seconds: i64, millis: i64) -> Time {
    start
        + chrono::Duration::minutes(minutes)
        + chrono::Duration::seconds(seconds)
        + chrono::Duration::milliseconds(millis)
}

/// The native driver computed its deadline for the published alarm at the
/// current reading, so a later UTC move cannot make it fire on its own.
async fn driver_settled(fixture: &Fixture, runtime: &Runtime) {
    if let Some(alarm) = alarm_ms(runtime) {
        fixture
            .clock
            .not_due_check_now(
                fixture
                    .clock
                    .wall_deadline(runtime_primitives::SyncTime::from_unix_millis(alarm)),
            )
            .await;
    }
}

/// Starts one due batch at `time` through the event path.
async fn due_batch_at(fixture: &Fixture, runtime: &Runtime, time: Time) {
    driver_settled(fixture, runtime).await;
    fixture.set_utc(time).await;
    runtime.after_event().await.unwrap();
    driver_settled(fixture, runtime).await;
}

fn fired() -> ProbeAnswer {
    EngineRequestOutcome::Answer {
        receipt: EngineReceipt::Applied {
            state: ProbeState::Fired,
        },
    }
}

#[tokio::test]
async fn a_probe_request_starts_the_due_batch_without_an_alarm_callback() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    due_batch_at(&fixture, &runtime, at(start, 4, 20, 0)).await;
    let (due_item, due_group) = (probe_id(12000), probe_id(12001));
    let (request_item, request_group) = (probe_id(12002), probe_id(12003));
    seed_probe(
        &runtime,
        due_item,
        due_group,
        Some(at(start, 5, 0, 0)),
        "due",
    )
    .await;
    seed_probe(&runtime, request_item, request_group, None, "request").await;
    driver_settled(&fixture, &runtime).await;
    let callbacks = runtime.runtime().alarm_completion_count();
    fixture.set_utc(at(start, 4, 58, 0)).await;

    let answer = runtime
        .request_probe(
            &fixture.caller,
            probe_request(12004, request_item, ProbeEvent::Run),
        )
        .await
        .unwrap();

    assert_eq!(answer, fired(), "the request is answered");
    assert_eq!(
        probe_facts(&runtime, due_item, due_group).await,
        vec![text("fired"), integer(1), integer(0)],
        "the request's own event ran the due batch 38 s after the last one"
    );
    assert_eq!(
        runtime.runtime().alarm_completion_count(),
        callbacks,
        "no alarm callback ran"
    );
    assert_eq!(alarm_ms(&runtime), None);
    fixture.host.close().await;
}

#[tokio::test]
async fn a_burst_of_forty_requests_runs_only_the_first_events_due_batch() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    let (first_due, second_due) = (probe_id(12100), probe_id(12101));
    let (first_group, second_group) = (probe_id(12102), probe_id(12103));
    let request_group = probe_id(12104);
    seed_probe(
        &runtime,
        first_due,
        first_group,
        Some(at(start, 5, 0, 0)),
        "first",
    )
    .await;
    // Only a second batch started at 10:04:58 or later reaches 10:05:48.
    seed_probe(
        &runtime,
        second_due,
        second_group,
        Some(at(start, 5, 48, 0)),
        "second",
    )
    .await;
    let requests: Vec<_> = (0..40).map(|offset| probe_id(12110 + offset)).collect();
    for (offset, item) in requests.iter().enumerate() {
        seed_probe(
            &runtime,
            *item,
            request_group,
            None,
            &format!("burst{offset}"),
        )
        .await;
    }

    for (offset, item) in requests.iter().enumerate() {
        let offset = i64::try_from(offset).unwrap();
        driver_settled(&fixture, &runtime).await;
        fixture.set_utc(at(start, 4, 57, 150 * offset)).await;
        let answer = runtime
            .request_probe(
                &fixture.caller,
                probe_request(
                    12200 + u32::try_from(offset).unwrap(),
                    *item,
                    ProbeEvent::Run,
                ),
            )
            .await
            .unwrap();
        assert_eq!(answer, fired(), "request {offset}");
        if offset == 0 {
            assert_eq!(
                probe_facts(&runtime, first_due, first_group).await,
                vec![text("fired"), integer(1), integer(0)],
                "the first event's due batch fired the 10:05:00 item"
            );
        }
    }
    driver_settled(&fixture, &runtime).await;
    fixture.set_utc(at(start, 5, 3, 0)).await;

    assert_eq!(
        probe_facts(&runtime, first_due, first_group).await,
        vec![text("fired"), integer(1), integer(0)],
        "the 10:05:00 item fired exactly once"
    );
    assert_eq!(
        probe_facts(&runtime, second_due, second_group).await,
        vec![text("waiting"), integer(0), integer(0)],
        "no second due batch ran inside the burst"
    );
    assert_eq!(
        alarm_ms(&runtime),
        Some(at(start, 5, 48, 0).timestamp_millis())
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn the_look_ahead_includes_fifty_seconds_and_excludes_fifty_one() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    let (inside, outside, group) = (probe_id(12300), probe_id(12301), probe_id(12302));
    seed_probe(&runtime, inside, group, Some(at(start, 5, 0, 0)), "inside").await;
    seed_probe(
        &runtime,
        outside,
        group,
        Some(at(start, 5, 1, 0)),
        "outside",
    )
    .await;

    due_batch_at(&fixture, &runtime, at(start, 4, 10, 0)).await;

    assert_eq!(
        probe_facts(&runtime, inside, group).await,
        vec![text("fired"), integer(1), integer(0)]
    );
    assert_eq!(
        probe_facts(&runtime, outside, group).await,
        vec![text("waiting"), integer(1), integer(0)]
    );
    assert_eq!(
        alarm_ms(&runtime),
        Some(at(start, 5, 1, 0).timestamp_millis()),
        "the excluded item is the alarm, past the 10:04:40 floor"
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn the_fallback_alarm_waits_for_the_floor_and_then_runs_the_item() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    due_batch_at(&fixture, &runtime, at(start, 4, 58, 0)).await;
    let (item, group) = (probe_id(12400), probe_id(12401));
    seed_probe(&runtime, item, group, Some(at(start, 5, 10, 0)), "after").await;

    // A second event inside the spacing only publishes.
    runtime.after_event().await.unwrap();

    assert_eq!(
        alarm_ms(&runtime),
        Some(at(start, 5, 28, 0).timestamp_millis()),
        "10:05:10 is raised to the 10:05:28 floor"
    );
    assert_eq!(
        probe_facts(&runtime, item, group).await,
        vec![text("waiting"), integer(0), integer(0)]
    );
    due_batch_at(&fixture, &runtime, at(start, 5, 28, 0)).await;
    assert_eq!(
        probe_facts(&runtime, item, group).await,
        vec![text("fired"), integer(1), integer(0)]
    );
    assert_eq!(alarm_ms(&runtime), None, "nothing is pending afterwards");
    fixture.host.close().await;
}

#[tokio::test]
async fn a_probe_item_without_a_due_time_never_arms_the_alarm() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    let (traffic, group) = (probe_id(12500), probe_id(12501));
    seed_probe(&runtime, traffic, group, None, "traffic").await;

    due_batch_at(&fixture, &runtime, at(start, 1, 0, 0)).await;

    assert_eq!(alarm_ms(&runtime), None, "a traffic-driven item has no due");
    let timed = probe_id(12502);
    seed_probe(&runtime, timed, group, Some(at(start, 15, 0, 0)), "timed").await;
    due_batch_at(&fixture, &runtime, at(start, 2, 0, 0)).await;
    assert_eq!(
        alarm_ms(&runtime),
        Some(at(start, 15, 0, 0).timestamp_millis()),
        "only the timed item sets the alarm"
    );
    assert_eq!(
        probe_facts(&runtime, traffic, group).await,
        vec![text("waiting"), integer(0), integer(0)]
    );
    fixture.host.close().await;
}
