//! V2 checks 1-4 on the native host: for wakes and automations, each corrupt
//! subject next to a healthy due one fails alone (count 1, next try the next
//! mark), retires on the next pass and is never visited again.
use super::super::engine_v2_support::*;
use super::super::support::*;
use router_clock::RuntimeClock;

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
    let observations = EngineObservations::default();
    let _capture = observations.install();
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let first_mark = next_mark(fixture.clock.utc_now());
    let second_mark = first_mark + minutes(5);

    let healthy_wake = set_wake(&fixture, 4000, first_mark, "healthy wake").await;
    let healthy_automation = set_automation_at(&fixture, 4001, first_mark, "healthy").await;
    let wake_row = "SELECT * FROM wakes WHERE wake_id=?";
    let automation_row = "SELECT * FROM automations WHERE automation_id=?";
    let corrupt = [
        Corrupt {
            kind: "wake_timer",
            key: set_wake(&fixture, 4002, first_mark, "bad target").await,
            column: "wakes.target",
            table_row: wake_row,
        },
        Corrupt {
            kind: "wake_timer",
            key: set_wake(&fixture, 4003, first_mark, "bad due").await,
            column: "wakes.due_at",
            table_row: wake_row,
        },
        Corrupt {
            kind: "wake_timer",
            key: set_wake(&fixture, 4004, first_mark, "bad state").await,
            column: "wakes.state",
            table_row: wake_row,
        },
        Corrupt {
            kind: "automation_timer",
            key: set_automation_at(&fixture, 4005, first_mark, "bad timing").await,
            column: "automations.timing_rule",
            table_row: automation_row,
        },
    ];
    raw(
        &runtime,
        "UPDATE wakes SET target='not-json' WHERE wake_id=?",
        vec![text(&corrupt[0].key)],
    )
    .await;
    raw(
        &runtime,
        "UPDATE wakes SET due_at='not-a-time' WHERE wake_id=?",
        vec![text(&corrupt[1].key)],
    )
    .await;
    raw(
        &runtime,
        "UPDATE wakes SET state='damaged' WHERE wake_id=?",
        vec![text(&corrupt[2].key)],
    )
    .await;
    raw(
        &runtime,
        "UPDATE automations SET timing_rule='not-json' WHERE automation_id=?",
        vec![text(&corrupt[3].key)],
    )
    .await;

    // Pass 1 at the first mark.
    fixture.clock.set_utc(first_mark);
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
        let observed = observations.for_key(&subject.key);
        assert_eq!(observed.len(), 1, "{}: one observation", subject.column);
        assert_eq!(
            (
                observed[0].kind.as_str(),
                observed[0].class.as_str(),
                observed[0].count,
                observed[0].retired
            ),
            (subject.kind, "corrupt_row", 1, false)
        );
    }
    assert_eq!(
        alarm_ms(&runtime),
        Some(second_mark.timestamp_millis()),
        "alarm is the next mark"
    );
    let messages_after_first = all_messages(&runtime).await;

    // Pass 2 at the next mark: every record retires at count 2, nothing new is sent.
    fixture.clock.set_utc(second_mark);
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
        let observed = observations.for_key(&subject.key);
        assert_eq!(
            observed.len(),
            2,
            "{}: the retirement is observed",
            subject.column
        );
        assert_eq!((observed[1].count, observed[1].retired), (2, true));
    }
    assert_eq!(
        all_messages(&runtime).await,
        messages_after_first,
        "no new messages"
    );
    assert_eq!(alarm_ms(&runtime), None, "nothing else is pending");

    // An hour of marks: no Due reaches a retired subject.
    let mut before = Vec::new();
    for subject in &corrupt {
        before.push((
            subject_row(&runtime, subject).await,
            whole_record(&runtime, subject.kind, &subject.key).await,
        ));
    }
    let observed_before = observations.failures().len();
    for step in 1..=12 {
        fixture.clock.set_utc(second_mark + minutes(5 * step));
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
    assert_eq!(
        observations.failures().len(),
        observed_before,
        "no further failure observed"
    );
    assert_eq!(all_messages(&runtime).await, messages_after_first);
    fixture.host.close().await;
}
