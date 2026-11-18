//! V5 checks 2 and 4 on the native host, read back from the physical alarm:
//! 600 real wakes drain 250 at a time, an only-unreadable wake keeps a wake-up
//! before any batch has failed it, a Failing record with an out-of-range retry
//! still gets visited, and the alarm query runs with six registered kinds.
//! Times are named by their wall time from the 10:00:00 start.
use super::super::engine_v2_support::*;
use super::super::{Statement, support::*};
use super::engine_alarm::{driver_settled, due_batch_at};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, StatementBatch,
};
use router_clock::RuntimeClock;

fn at(start: Time, minutes: i64, seconds: i64) -> Time {
    start + chrono::Duration::minutes(minutes) + chrono::Duration::seconds(seconds)
}
fn ms(time: Time) -> Option<i64> {
    Some(time.timestamp_millis())
}
/// Keys run opposite to due order, so key order cannot pass for earliest first.
fn bulk_wake_id(number: usize) -> String {
    format!("01890f2e-7b4c-7cc0-98c4-{:012}", 30_599 - number)
}
/// Publishes the alarm at `time` without a due batch (the request path's own
/// recompute), with the last batch start far enough back that its 30 s floor
/// is already past; the claim snapshot proves no batch started.
async fn publish_without_a_batch_at(fixture: &Fixture, runtime: &Runtime, time: Time) {
    let before = runtime.due_batch_snapshot().last_start_unix_millis;
    assert!(
        before.is_some_and(|start| time.timestamp_millis() - start >= 30_000),
        "the last batch floor must already be past: {before:?}"
    );
    driver_settled(fixture, runtime).await;
    fixture.set_utc(time).await;
    runtime.recompute_alarm().await.unwrap();
    assert_eq!(
        runtime.due_batch_snapshot().last_start_unix_millis,
        before,
        "no due batch started"
    );
}
async fn done_wakes(runtime: &Runtime) -> i64 {
    count(
        runtime,
        "SELECT COUNT(*) FROM wakes WHERE state='done'",
        vec![],
    )
    .await
}
/// Done wakes due at or before `through`: equals the done count only when the
/// batches took the earliest first.
async fn done_wakes_due_by(runtime: &Runtime, through: Time) -> i64 {
    count(
        runtime,
        "SELECT COUNT(*) FROM wakes WHERE state='done' AND due_at<=?",
        vec![stamp(through)],
    )
    .await
}

#[tokio::test]
async fn six_hundred_due_wakes_drain_earliest_first_in_capped_batches_thirty_seconds_apart() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    // Wake i is due on its own past mark, earliest first: 599 is the 10:05
    // wake the request writes; each stored row keeps due > created_at.
    let due = |number: usize| {
        at(start, 5, 0) - chrono::Duration::minutes(5 * (599 - i64::try_from(number).unwrap()))
    };
    let template = set_wake(&fixture, 14000, due(599), "bulk wake").await;
    let statements = (0..599)
        .map(|number| GuardedStatement {
            statement: Statement {
                sql: "INSERT INTO wakes(wake_id,target,due_at,text,state,created_at,updated_at) SELECT ?1,target,?2,text,'active',?3,?3 FROM wakes WHERE wake_id=?4",
                args: vec![
                    text(bulk_wake_id(number)),
                    stamp(due(number)),
                    stamp(due(number) - chrono::Duration::minutes(1)),
                    text(&template),
                ],
            },
            expect: RowExpectation::Exactly(1),
        })
        .collect();
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements,
                alarm: AlarmUpdate::Unchanged,
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    assert_eq!(
        count(
            &runtime,
            "SELECT COUNT(*) FROM wakes WHERE state='active'",
            vec![]
        )
        .await,
        600
    );

    due_batch_at(&fixture, &runtime, at(start, 5, 0)).await;
    assert_eq!(done_wakes(&runtime).await, 250);
    assert_eq!(
        done_wakes_due_by(&runtime, due(249)).await,
        250,
        "earliest first"
    );
    assert_eq!(alarm_ms(&runtime), ms(at(start, 5, 30)), "continuation");

    due_batch_at(&fixture, &runtime, at(start, 5, 30)).await;
    assert_eq!(done_wakes(&runtime).await, 500);
    assert_eq!(
        done_wakes_due_by(&runtime, due(499)).await,
        500,
        "earliest first"
    );
    assert_eq!(alarm_ms(&runtime), ms(at(start, 6, 0)));

    due_batch_at(&fixture, &runtime, at(start, 6, 0)).await;
    assert_eq!(done_wakes(&runtime).await, 600);
    assert_eq!(
        count(
            &runtime,
            "SELECT COUNT(*) FROM messages WHERE message_kind='wake'",
            vec![]
        )
        .await,
        600,
        "each wake fired once"
    );
    assert_eq!(alarm_ms(&runtime), None);
    fixture.host.close().await;
}

#[tokio::test]
async fn an_only_unreadable_wake_publishes_the_next_mark_before_any_batch_fails_it() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    fixture.set_utc(at(start, 1, 30)).await;
    // The request's own due batch starts at 10:01:30.
    let wake = set_wake(&fixture, 14100, at(start, 15, 0), "unreadable").await;
    assert_eq!(
        runtime.due_batch_snapshot().last_start_unix_millis,
        ms(at(start, 1, 30))
    );
    assert_eq!(alarm_ms(&runtime), ms(at(start, 15, 0)));
    raw(
        &runtime,
        "UPDATE wakes SET due_at='not-a-time' WHERE wake_id=?",
        vec![text(&wake)],
    )
    .await;

    publish_without_a_batch_at(&fixture, &runtime, at(start, 2, 10)).await;

    assert_eq!(alarm_ms(&runtime), ms(at(start, 5, 0)), "never none");
    assert_eq!(
        record(&runtime, "wake_timer", &wake).await,
        Vec::<Vec<SqlValue>>::new()
    );
    assert_eq!(
        rows(
            &runtime,
            "SELECT state FROM wakes WHERE wake_id=?",
            vec![text(&wake)]
        )
        .await,
        vec![vec![text("active")]]
    );

    due_batch_at(&fixture, &runtime, at(start, 5, 0)).await;

    assert_eq!(
        record(&runtime, "wake_timer", &wake).await,
        vec![vec![
            text("failing"),
            integer(1),
            stamp(at(start, 10, 0)),
            text("corrupt_row"),
            text("wakes.due_at")
        ]]
    );
    assert_eq!(alarm_ms(&runtime), ms(at(start, 10, 0)));
    fixture.host.close().await;
}

#[tokio::test]
async fn a_failing_record_with_an_out_of_range_retry_publishes_the_next_mark_and_is_visited() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    fixture.set_utc(at(start, 1, 30)).await;
    let wake = set_wake(&fixture, 14200, at(start, 15, 0), "retry out of range").await;
    let planted = "01890f2e-7b4c-7cc0-98c4-000000014201";
    raw(
        &runtime,
        "INSERT INTO central_work_attempts(attempt_id,machine_kind,subject_key,state,failure_count,next_try_at,last_failure,last_failure_detail,first_failed_at,last_failed_at,retired_at,created_at,updated_at) VALUES (?,'wake_timer',?,'failing',1,'2262-04-11T23:47:17.854Z','statement_rejected',NULL,?,?,NULL,?,?)",
        vec![
            text(planted),
            text(&wake),
            stamp(at(start, 1, 30)),
            stamp(at(start, 1, 30)),
            stamp(at(start, 1, 30)),
            stamp(at(start, 1, 30)),
        ],
    )
    .await;

    publish_without_a_batch_at(&fixture, &runtime, at(start, 2, 10)).await;

    assert_eq!(
        alarm_ms(&runtime),
        ms(at(start, 5, 0)),
        "the next mark, never the 2262 retry"
    );

    due_batch_at(&fixture, &runtime, at(start, 5, 0)).await;

    let visited = (
        rows(
            &runtime,
            "SELECT state FROM wakes WHERE wake_id=?",
            vec![text(&wake)],
        )
        .await,
        wake_messages(&runtime, &wake).await,
        record(&runtime, "wake_timer", &wake).await,
    );
    assert_eq!(
        visited,
        (
            vec![vec![text("active")]],
            0,
            vec![vec![
                text("failing"),
                integer(1),
                stamp(at(start, 10, 0)),
                text("corrupt_row"),
                text("central_work_attempts.next_try_at\n2262-04-11T23:47:17.854Z")
            ]]
        ),
        "the 10:05 batch visited the subject and replaced its unreadable record"
    );
    assert_eq!(alarm_ms(&runtime), ms(at(start, 10, 0)));
    fixture.host.close().await;
}

#[tokio::test]
async fn the_alarm_query_runs_with_six_registered_kinds_and_the_sixth_contributes() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    raw(
        &runtime,
        "INSERT INTO engine_alarm_only_items(item_id,due_at,state,created_at,updated_at) VALUES ('01890f2e-7b4c-7cc0-98c4-000000014300',?,'waiting',?,?)",
        vec![stamp(at(start, 15, 0)), stamp(start), stamp(start)],
    )
    .await;

    due_batch_at(&fixture, &runtime, at(start, 1, 0)).await;
    assert_eq!(
        alarm_ms(&runtime),
        ms(at(start, 15, 0)),
        "the sixth kind's due"
    );

    raw(&runtime, "DELETE FROM engine_alarm_only_items", vec![]).await;
    due_batch_at(&fixture, &runtime, at(start, 2, 0)).await;
    assert_eq!(alarm_ms(&runtime), None);
    fixture.host.close().await;
}
