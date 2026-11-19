//! V2 checks 1-4 on the workerd host: for wakes and automations, each corrupt
//! subject next to a healthy due one fails alone (count 1, next try the next
//! mark), retires on the next pass and is never visited again. Observations are
//! not asserted here: the plain workerd fixture (WorkerdHost::open) prints
//! Worker logs to stderr and captures none; the sign-in fixture captures them
//! and engine_admin.rs asserts the same observation lines there.
use super::super::engine_evidence::{
    assert_fixture_attempt_ids_are_uuid_v7, witness_fixture_attempt_ids,
};
use super::super::engine_v2_support::*;
use super::super::support::*;

struct Corrupt {
    kind: &'static str,
    key: String,
    column: &'static str,
    table_row: &'static str,
}

async fn subject_row(runtime: &Runtime, corrupt: &Corrupt) -> Vec<Vec<central_storage::SqlValue>> {
    rows(runtime, corrupt.table_row, vec![text(&corrupt.key)]).await
}

#[tokio::test]
async fn each_corrupt_timer_fails_alone_retires_on_the_next_pass_and_is_never_visited_again() {
    let fixture = Fixture::new().await;
    witness_fixture_attempt_ids(&fixture).await;
    let runtime = runtime(&fixture).await;
    let first_mark = next_mark(now(&fixture));
    let second_mark = first_mark + minutes(5);

    let healthy_wake = set_wake(&fixture, 4000, first_mark, "healthy wake").await;
    let healthy_automation = set_automation_at(&fixture, 4001, first_mark, "healthy").await;
    let wake_row = "SELECT * FROM automation_wakes WHERE wake_id=?";
    let automation_row = "SELECT * FROM automation_schedules WHERE automation_id=?";
    let corrupt = [
        Corrupt {
            kind: "wake_timer",
            key: set_wake(&fixture, 4002, first_mark, "bad target").await,
            column: "automation_wakes.target",
            table_row: wake_row,
        },
        Corrupt {
            kind: "wake_timer",
            key: set_wake(&fixture, 4003, first_mark, "bad due").await,
            column: "automation_wakes.due_at",
            table_row: wake_row,
        },
        Corrupt {
            kind: "wake_timer",
            key: set_wake(&fixture, 4004, first_mark, "bad state").await,
            column: "automation_wakes.state",
            table_row: wake_row,
        },
        Corrupt {
            kind: "automation_timer",
            key: set_automation_at(&fixture, 4005, first_mark, "bad timing").await,
            column: "automation_schedules.timing_rule",
            table_row: automation_row,
        },
    ];
    raw(
        &runtime,
        "UPDATE automation_wakes SET target='not-json' WHERE wake_id=?",
        vec![text(&corrupt[0].key)],
    )
    .await;
    raw(
        &runtime,
        "UPDATE automation_wakes SET due_at='not-a-time' WHERE wake_id=?",
        vec![text(&corrupt[1].key)],
    )
    .await;
    raw(
        &runtime,
        "UPDATE automation_wakes SET state='damaged' WHERE wake_id=?",
        vec![text(&corrupt[2].key)],
    )
    .await;
    raw(
        &runtime,
        "UPDATE automation_schedules SET timing_rule='not-json' WHERE automation_id=?",
        vec![text(&corrupt[3].key)],
    )
    .await;

    // Pass 1 at the first mark.
    set_now(&fixture, first_mark);
    runtime.after_event().await.unwrap();

    assert_eq!(
        wake_messages(&runtime, &healthy_wake).await,
        1,
        "healthy wake fires once"
    );
    assert_eq!(
        automation_messages(&runtime, &healthy_automation).await,
        1,
        "healthy automation fires once"
    );
    for subject in &corrupt {
        let found = record(&runtime, subject.kind, &subject.key).await;
        assert_eq!(found.len(), 1, "{}: one record", subject.column);
        assert_eq!(
            &found[0][..4],
            &[
                text("failing"),
                integer(1),
                stamp(second_mark),
                text("corrupt_row")
            ],
            "{}",
            subject.column
        );
        assert!(
            matches!(&found[0][4], central_storage::SqlValue::Text(detail) if detail.contains(subject.column)),
            "{}: detail names the column: {:?}",
            subject.column,
            found[0][4]
        );
    }
    assert_eq!(
        alarm_ms(&runtime).await,
        Some(second_mark.timestamp_millis()),
        "alarm is the next mark"
    );
    let messages_after_first = all_messages(&runtime).await;

    // Pass 2 at the next mark: every record retires at count 2, nothing new is sent.
    set_now(&fixture, second_mark);
    runtime.after_event().await.unwrap();

    for subject in &corrupt {
        let found = record(&runtime, subject.kind, &subject.key).await;
        assert_eq!(
            &found[0][..2],
            &[text("retired"), integer(2)],
            "{}",
            subject.column
        );
        assert_eq!(
            found[0][2],
            central_storage::SqlValue::Null,
            "a retired record has no next try"
        );
    }
    assert_eq!(
        all_messages(&runtime).await,
        messages_after_first,
        "no new messages"
    );
    assert_eq!(alarm_ms(&runtime).await, None, "nothing else is pending");

    // An hour of marks: no Due reaches a retired subject.
    let mut before = Vec::new();
    for subject in &corrupt {
        before.push((
            subject_row(&runtime, subject).await,
            whole_record(&runtime, subject.kind, &subject.key).await,
        ));
    }
    for step in 1..=12 {
        set_now(&fixture, second_mark + minutes(5 * step));
        runtime.after_event().await.unwrap();
    }
    for (subject, (row, stored)) in corrupt.iter().zip(before) {
        assert_eq!(
            subject_row(&runtime, subject).await,
            row,
            "{}: row unchanged",
            subject.column
        );
        assert_eq!(
            whole_record(&runtime, subject.kind, &subject.key).await,
            stored,
            "{}: record unchanged",
            subject.column
        );
    }
    assert_eq!(all_messages(&runtime).await, messages_after_first);
    assert_fixture_attempt_ids_are_uuid_v7(
        &fixture,
        "each_corrupt_timer_fails_alone_retires_on_the_next_pass_and_is_never_visited_again",
        &[],
    )
    .await;
    fixture.host.close().await;
}
