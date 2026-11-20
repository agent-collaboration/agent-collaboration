//! V2 on the native host: a corrupt drain never pulls the alarm off the grid,
//! and the drain unit's control row gates the due batch and every
//! request-triggered drain, across a restart.
use super::super::engine_evidence::{
    assert_fixture_attempt_ids_are_uuid_v7, witness_fixture_attempt_ids,
};
use super::super::engine_v2_support::{
    alarm_ms, minutes, next_mark, raw, record, rows, runtime, set_wake, stamp, wake_messages,
};
use super::*;

const DRAINS: &str = "legacy_workspace_drains";

/// Corrupts the fixture workspace's drain cursor and makes the drain due at
/// `due`; returns the cursor for the repair.
async fn corrupt_drain(f: &Fixture, due: chrono::DateTime<chrono::Utc>) -> SqlValue {
    let runtime = runtime(f).await;
    let cursor = rows(
        &runtime,
        "SELECT drained_through_position FROM mail_workspace_drains WHERE workspace_id=?",
        vec![text(&f.workspace)],
    )
    .await[0][0]
        .clone();
    raw(
        &runtime,
        "UPDATE mail_workspace_drains SET drained_through_position='bad',drain_due_at=? WHERE workspace_id=?",
        vec![stamp(due), text(&f.workspace)],
    )
    .await;
    cursor
}
async fn repair_drain(f: &Fixture, cursor: SqlValue) {
    raw(
        &runtime(f).await,
        "UPDATE mail_workspace_drains SET drained_through_position=? WHERE workspace_id=?",
        vec![cursor, text(&f.workspace)],
    )
    .await;
}
async fn drain_due(f: &Fixture) -> SqlValue {
    rows(
        &runtime(f).await,
        "SELECT drain_due_at FROM mail_workspace_drains WHERE workspace_id=?",
        vec![text(&f.workspace)],
    )
    .await[0][0]
        .clone()
}

#[tokio::test]
async fn a_failing_drain_arms_its_next_try_on_a_mark_never_its_past_drain_due() {
    let f = Fixture::new().await;
    witness_fixture_attempt_ids(&f).await;
    f.area(8400).await;
    let first = next_mark(f.clock.utc_now());
    let past_due = f.clock.utc_now() - chrono::Duration::seconds(7);
    corrupt_drain(&f, past_due).await;
    let wake = set_wake(&f, 8401, first, "unaffected wake").await;
    let runtime = runtime(&f).await;

    for pass in 0..3 {
        let now = first + minutes(5 * pass);
        f.set_utc(now).await;
        runtime.after_event().await.unwrap();
        let found = record(&runtime, DRAINS, "all").await;
        let next_try = now + minutes(5);
        assert_eq!(
            &found[0][..4],
            &[
                text("failing"),
                SqlValue::Integer((pass + 1).try_into().unwrap()),
                stamp(next_try),
                text("corrupt_row")
            ]
        );
        let alarm = alarm_ms(&runtime).unwrap();
        assert_eq!(
            alarm,
            next_try.timestamp_millis(),
            "the alarm is the next try"
        );
        assert_eq!(alarm % 300_000, 0, "on a mark");
        assert_ne!(
            alarm,
            past_due.timestamp_millis(),
            "never the past drain_due_at"
        );
        assert_eq!(
            drain_due(&f).await,
            stamp(past_due),
            "the drain keeps its due time"
        );
    }
    assert_eq!(wake_messages(&runtime, &wake).await, 1, "wakes unaffected");
    assert_fixture_attempt_ids_are_uuid_v7(
        &f,
        "a_failing_drain_arms_its_next_try_on_a_mark_never_its_past_drain_due",
        &[],
    )
    .await;
    f.host.close().await;
}

#[tokio::test]
async fn a_failing_drain_unit_gates_due_batches_and_request_drains_until_its_next_try() {
    let mut f = Fixture::new().await;
    witness_fixture_attempt_ids(&f).await;
    f.area(8410).await;
    let failure = next_mark(f.clock.utc_now());
    let cursor = corrupt_drain(&f, failure - chrono::Duration::seconds(1)).await;
    let runtime = runtime(&f).await;
    f.set_utc(failure).await;
    runtime.after_event().await.unwrap();
    assert_eq!(
        record(&runtime, DRAINS, "all").await[0][..3],
        [
            text("failing"),
            SqlValue::Integer(1.try_into().unwrap()),
            stamp(failure + minutes(5))
        ]
    );
    let gated = runtime.runtime().legacy_unit_invocations();
    let backstop = drain_due(&f).await;

    // Two workspace writes start due batches before the next try; a restart between.
    f.set_utc(failure + minutes(1)).await;
    created(
        f.call(
            8411,
            m::WorkspaceOperation::CreateArea {
                name: name("Gated one"),
                description: "Test description".to_owned().try_into().unwrap(),
            },
        )
        .await,
    );
    let after_first = runtime.runtime().legacy_unit_invocations();
    assert_eq!(
        (after_first.drains, after_first.request_drains),
        (gated.drains, gated.request_drains),
        "neither the due batch nor the request drain runs the gated unit"
    );
    drop(runtime);
    f.reopen().await;
    let runtime = super::super::engine_v2_support::runtime(&f).await;
    f.set_utc(failure + minutes(2)).await;
    created(
        f.call(
            8412,
            m::WorkspaceOperation::CreateArea {
                name: name("Gated two"),
                description: "Test description".to_owned().try_into().unwrap(),
            },
        )
        .await,
    );
    let after_second = runtime.runtime().legacy_unit_invocations();
    assert_eq!(
        (after_second.drains, after_second.request_drains),
        (0, 0),
        "after the restart the stored row still gates both paths"
    );
    assert!(
        matches!(drain_due(&f).await, SqlValue::Text(_)),
        "the backstop stays"
    );
    assert_eq!(drain_due(&f).await, backstop);
    assert_eq!(
        record(&runtime, DRAINS, "all").await[0][..2],
        [text("failing"), SqlValue::Integer(1.try_into().unwrap())],
        "still gated by the stored row"
    );

    repair_drain(&f, cursor).await;
    f.set_utc(failure + minutes(5)).await;
    runtime.after_event().await.unwrap();
    assert_eq!(
        runtime.runtime().legacy_unit_invocations().drains,
        1,
        "runs once at its next try"
    );
    assert_eq!(drain_due(&f).await, SqlValue::Null, "and drains");
    assert!(record(&runtime, DRAINS, "all").await.is_empty());
    assert_fixture_attempt_ids_are_uuid_v7(
        &f,
        "a_failing_drain_unit_gates_due_batches_and_request_drains_until_its_next_try",
        &[],
    )
    .await;
    f.host.close().await;
}

#[tokio::test]
async fn a_failing_request_drain_records_against_the_unit_and_keeps_its_committed_answer() {
    let f = Fixture::new().await;
    witness_fixture_attempt_ids(&f).await;
    f.area(8420).await;
    let runtime = runtime(&f).await;
    // Ten seconds before a mark, so the next try (the mark) comes before the
    // next due batch may start.
    let mark = next_mark(f.clock.utc_now() + minutes(1));
    let at = mark - chrono::Duration::seconds(10);
    f.set_utc(at).await;
    let cursor = corrupt_drain(&f, at + minutes(60)).await;
    let before = runtime.runtime().legacy_unit_invocations();

    let reply = f
        .call(
            8421,
            m::WorkspaceOperation::CreateArea {
                name: name("Committed"),
                description: "Test description".to_owned().try_into().unwrap(),
            },
        )
        .await;

    assert!(
        matches!(
            reply.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
        ),
        "the committed answer is returned: {reply:?}"
    );
    assert_eq!(
        runtime.runtime().legacy_unit_invocations().request_drains,
        before.request_drains + 1
    );
    let found = record(&runtime, DRAINS, "all").await;
    assert_eq!(
        &found[0][..4],
        &[
            text("failing"),
            SqlValue::Integer(1.try_into().unwrap()),
            stamp(mark),
            text("corrupt_row")
        ]
    );
    let whole = rows(
        &runtime,
        "SELECT * FROM engine_work_attempts WHERE machine_kind=? AND subject_key='all'",
        vec![text(DRAINS)],
    )
    .await;

    // Healthy again, its next try reached, within 30 s of the last batch: the
    // request drain succeeds for its workspace and leaves the unit's record.
    repair_drain(&f, cursor).await;
    f.set_utc(mark + chrono::Duration::seconds(1)).await;
    let reply = f
        .call(
            8422,
            m::WorkspaceOperation::CreateArea {
                name: name("Drained"),
                description: "Test description".to_owned().try_into().unwrap(),
            },
        )
        .await;
    assert!(matches!(reply.result, m::WorkspaceResult::Done(_)));
    assert_eq!(
        runtime.runtime().legacy_unit_invocations().request_drains,
        before.request_drains + 2
    );
    assert_eq!(
        drain_due(&f).await,
        SqlValue::Null,
        "the request drained its workspace"
    );
    assert_eq!(
        rows(
            &runtime,
            "SELECT * FROM engine_work_attempts WHERE machine_kind=? AND subject_key='all'",
            vec![text(DRAINS)],
        )
        .await,
        whole,
        "one workspace's success leaves the unit's record unchanged"
    );
    assert_fixture_attempt_ids_are_uuid_v7(
        &f,
        "a_failing_request_drain_records_against_the_unit_and_keeps_its_committed_answer",
        &[],
    )
    .await;
    f.host.close().await;
}
