//! V5 and V6 on workerd: the retained alarm for each pending shape, and which
//! failures fail the real alarm handler. Times are named from the 10:00:00 start.
use super::super::{Statement, support::*};
use super::engine_probe_support::*;
use central_storage::{SqlValue, StoreFailure};
use collaboration_central_core::engine_probe::{EngineRequestOutcome, ProbeEvent};
use fleet_runtime::EngineReadFault;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
};

type Time = chrono::DateTime<chrono::Utc>;

fn at(start: Time, minutes: i64, seconds: i64) -> Time {
    start + chrono::Duration::minutes(minutes) + chrono::Duration::seconds(seconds)
}
fn ms(time: Time) -> Option<i64> {
    Some(time.timestamp_millis())
}
/// Starts one due batch at `time` through the real alarm callback.
async fn due_batch_at(fixture: &Fixture, runtime: &Runtime, time: Time) {
    set_now(fixture, time);
    runtime.after_event().await.unwrap();
}
/// Fires the retained alarm at `time`; it must be due exactly then.
async fn retained_alarm_fires_at(fixture: &Fixture, runtime: &Runtime, time: Time) {
    assert_eq!(alarm_ms(runtime).await, ms(time));
    set_now(fixture, time);
    assert!(runtime.dispatch_due_alarm().await.unwrap().is_some());
}
async fn set_wake(fixture: &Fixture, number: u32, due: Time) -> workspace_ids::WakeId {
    let CentralMailResult::Done(CentralMailAnswer::WakeSet { wake_id, .. }) = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(number),
                operation: CentralMailOperation::SetWake {
                    target: session(&fixture.caller),
                    due_at: due,
                    text: "engine alarm".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("wake refused")
    };
    wake_id
}
async fn fired_in_group(runtime: &Runtime, group: uuid::Uuid) -> SqlValue {
    runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT COUNT(*) FROM engine_probe_items WHERE group_id=? AND state='fired'",
            args: vec![text(group)],
        })
        .await
        .unwrap()
        .rows[0][0]
        .clone()
}
async fn source_attempt(runtime: &Runtime, kind: &str) -> Vec<Vec<SqlValue>> {
    runtime.statement_host().read(Statement {
        sql: "SELECT state,failure_count,last_failure FROM central_work_attempts WHERE machine_kind='engine_source' AND subject_key=?",
        args: vec![text(kind)],
    }).await.unwrap().rows
}
async fn callbacks(runtime: &Runtime) -> u64 {
    runtime.control("raw-alarm-count", &()).await.unwrap()
}

#[tokio::test]
async fn nothing_pending_clears_the_alarm() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let (item, group) = (probe_id(13000), probe_id(13001));
    seed_probe(&runtime, item, group, Some(at(start, 10, 0)), "pending").await;
    due_batch_at(&fixture, &runtime, at(start, 1, 0)).await;
    assert_eq!(alarm_ms(&runtime).await, ms(at(start, 10, 0)));

    let stop = runtime
        .request_probe(
            &fixture.caller,
            probe_request(13002, item, ProbeEvent::Stop),
        )
        .await
        .unwrap();

    assert!(matches!(stop, EngineRequestOutcome::Answer { .. }));
    assert_eq!(alarm_ms(&runtime).await, None);
    fixture.host.close().await;
}

#[tokio::test]
async fn a_future_due_on_a_mark_is_kept() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    seed_probe(
        &runtime,
        probe_id(13010),
        probe_id(13011),
        Some(at(start, 10, 0)),
        "future",
    )
    .await;

    due_batch_at(&fixture, &runtime, at(start, 1, 0)).await;

    assert_eq!(alarm_ms(&runtime).await, ms(at(start, 10, 0)));
    fixture.host.close().await;
}

#[tokio::test]
async fn a_pending_due_exactly_now_publishes_the_next_mark() {
    let mut fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let (item, group) = (probe_id(13020), probe_id(13021));
    seed_probe(&runtime, item, group, Some(at(start, 5, 0)), "now").await;
    set_now(&fixture, at(start, 5, 0));

    fixture.reopen().await;

    let runtime = runtime_after_reopen(&fixture).await;
    assert_eq!(alarm_ms(&runtime).await, ms(at(start, 10, 0)));
    assert_eq!(
        probe_facts(&runtime, item, group).await,
        vec![text("waiting"), integer(0), integer(0)]
    );
    fixture.host.close().await;
}
/// The first ordinary control starts the fresh object, which recomputes its
/// alarm without a due batch; raw alarm reads alone do not start it.
async fn runtime_after_reopen(fixture: &Fixture) -> Runtime {
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT 1",
            args: vec![],
        })
        .await
        .unwrap();
    runtime
}

#[tokio::test]
async fn an_old_pending_due_publishes_the_next_mark() {
    let mut fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    seed_probe(
        &runtime,
        probe_id(13030),
        probe_id(13031),
        Some(at(start, 1, 0)),
        "old",
    )
    .await;
    set_now(&fixture, at(start, 17, 0));

    fixture.reopen().await;

    let runtime = runtime_after_reopen(&fixture).await;
    assert_eq!(alarm_ms(&runtime).await, ms(at(start, 20, 0)));
    fixture.host.close().await;
}

#[tokio::test]
async fn six_hundred_due_items_drain_in_capped_batches_thirty_seconds_apart() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let group = probe_id(13040);
    for offset in 0..600 {
        seed_probe(
            &runtime,
            probe_id(20000 + offset),
            group,
            Some(at(start, 5, 0)),
            &format!("bulk{offset}"),
        )
        .await;
    }

    due_batch_at(&fixture, &runtime, at(start, 5, 0)).await;
    assert_eq!(fired_in_group(&runtime, group).await, integer(250));

    retained_alarm_fires_at(&fixture, &runtime, at(start, 5, 30)).await;
    assert_eq!(fired_in_group(&runtime, group).await, integer(500));

    retained_alarm_fires_at(&fixture, &runtime, at(start, 6, 0)).await;
    assert_eq!(fired_in_group(&runtime, group).await, integer(600));
    assert_eq!(alarm_ms(&runtime).await, None);
    fixture.host.close().await;
}

#[tokio::test]
async fn an_only_unreadable_due_publishes_the_next_mark_never_none() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let (item, group) = (probe_id(13050), probe_id(13051));
    seed_probe(&runtime, item, group, Some(at(start, 5, 0)), "unreadable").await;
    raw(
        &runtime,
        "UPDATE engine_probe_items SET due_at='not-a-time' WHERE item_id=?",
        vec![text(item)],
    )
    .await;

    due_batch_at(&fixture, &runtime, at(start, 5, 0)).await;

    assert_eq!(alarm_ms(&runtime).await, ms(at(start, 10, 0)));
    assert_eq!(
        attempt(&runtime, item).await,
        vec![vec![text("failing"), integer(1), text("corrupt_row")]]
    );
    fixture.host.close().await;
}

/// One rejected due item: Failing after the first batch, Retired after the next.
async fn rejected_item(fixture: &Fixture, runtime: &Runtime, start: Time) -> uuid::Uuid {
    let (item, group) = (probe_id(13060), probe_id(13061));
    seed_probe(runtime, item, group, Some(at(start, 5, 0)), "rejected").await;
    raw(
        runtime,
        "CREATE TRIGGER probe_reject BEFORE UPDATE OF effect_count ON engine_probe_groups BEGIN SELECT RAISE(ABORT,'probe rejected'); END",
        vec![],
    )
    .await;
    due_batch_at(fixture, runtime, at(start, 5, 20)).await;
    item
}

#[tokio::test]
async fn a_failing_record_publishes_its_next_try_mark() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);

    let item = rejected_item(&fixture, &runtime, start).await;

    assert_eq!(
        attempt(&runtime, item).await,
        vec![vec![
            text("failing"),
            integer(1),
            text("statement_rejected")
        ]]
    );
    assert_eq!(
        alarm_ms(&runtime).await,
        ms(at(start, 10, 0)),
        "next_try_at, not the 10:05:50 floor"
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn a_retired_record_alone_clears_the_alarm() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let item = rejected_item(&fixture, &runtime, start).await;

    retained_alarm_fires_at(&fixture, &runtime, at(start, 10, 0)).await;

    assert_eq!(
        attempt(&runtime, item).await,
        vec![vec![
            text("retired"),
            integer(2),
            text("statement_rejected")
        ]]
    );
    assert_eq!(alarm_ms(&runtime).await, None);
    fixture.host.close().await;
}

#[tokio::test]
async fn a_request_wake_before_the_alarm_moves_it_in_its_own_transaction() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    set_now(&fixture, at(start, 1, 0));
    set_wake(&fixture, 13070, at(start, 30, 0)).await;
    assert_eq!(alarm_ms(&runtime).await, ms(at(start, 30, 0)));
    set_now(&fixture, at(start, 1, 10));
    // The event's own final publication fails, so only the request's
    // transaction can move the alarm.
    assert!(
        runtime
            .control::<bool>("fail-alarm-batch", &StoreFailure::Unavailable)
            .await
            .unwrap()
    );

    set_wake(&fixture, 13071, at(start, 20, 0)).await;

    assert_eq!(alarm_ms(&runtime).await, ms(at(start, 20, 0)));
    assert!(
        runtime.after_event().await.is_ok(),
        "the request's final publication consumed the injected failure"
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn a_corrupt_due_item_fails_alone_and_the_handler_succeeds() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let (corrupt, healthy, group) = (probe_id(13080), probe_id(13081), probe_id(13082));
    seed_probe(&runtime, corrupt, group, Some(at(start, 5, 0)), "corrupt").await;
    seed_probe(&runtime, healthy, group, Some(at(start, 5, 0)), "healthy").await;
    raw(
        &runtime,
        "UPDATE engine_probe_items SET state='damaged' WHERE item_id=?",
        vec![text(corrupt)],
    )
    .await;
    set_now(&fixture, at(start, 5, 0));

    let completion = runtime.after_event().await;

    assert!(completion.is_ok(), "{completion:?}");
    assert_eq!(
        probe_facts(&runtime, healthy, group).await,
        vec![text("fired"), integer(1), integer(0)]
    );
    assert_eq!(
        attempt(&runtime, corrupt).await,
        vec![vec![text("failing"), integer(1), text("corrupt_row")]]
    );
    assert_eq!(alarm_ms(&runtime).await, ms(at(start, 10, 0)));
    fixture.host.close().await;
}

#[tokio::test]
async fn a_failed_final_publication_fails_the_alarm_handler() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    let (due, later, group) = (probe_id(13090), probe_id(13091), probe_id(13092));
    seed_probe(&runtime, due, group, Some(at(start, 5, 0)), "due").await;
    seed_probe(&runtime, later, group, Some(at(start, 20, 0)), "later").await;
    set_now(
        &fixture,
        at(start, 5, 0) + chrono::Duration::milliseconds(500),
    );
    assert!(
        runtime
            .control::<bool>("fail-alarm-batch", &StoreFailure::Unavailable)
            .await
            .unwrap()
    );
    let before = callbacks(&runtime).await;

    let completion = runtime.after_event().await;

    assert_eq!(completion, Err(StoreFailure::Unavailable));
    assert!(callbacks(&runtime).await > before, "the real handler ran");
    assert_eq!(
        probe_facts(&runtime, due, group).await,
        vec![text("fired"), integer(1), integer(0)],
        "the batch's own unit committed"
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn a_failed_due_read_for_one_kind_still_handles_the_others() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = now(&fixture);
    set_now(&fixture, at(start, 0, 40));
    let wake = set_wake(&fixture, 13100, at(start, 5, 0)).await;
    let (item, group) = (probe_id(13101), probe_id(13102));
    seed_probe(&runtime, item, group, Some(at(start, 5, 0)), "unread").await;
    runtime
        .inject_engine_read_fault(EngineReadFault {
            kind: "engine_probe".into(),
        })
        .await;
    set_now(&fixture, at(start, 5, 0));

    let completion = runtime.after_event().await.unwrap();

    assert_eq!(completion.wakes_fired, 1);
    let wakes = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT state FROM wakes WHERE wake_id=?",
            args: vec![text(&wake)],
        })
        .await
        .unwrap();
    assert_eq!(wakes.rows, vec![vec![text("done")]]);
    assert_eq!(
        probe_facts(&runtime, item, group).await,
        vec![text("waiting"), integer(0), integer(0)],
        "the unread kind runs nothing"
    );
    assert_eq!(
        source_attempt(&runtime, "engine_probe").await,
        vec![vec![text("failing"), integer(1), text("corrupt_row")]]
    );
    assert_eq!(alarm_ms(&runtime).await, ms(at(start, 10, 0)));
    fixture.host.close().await;
}
