//! Paired with timing_contract's stored validation: an invalid timer or window row
//! no longer fails unrelated requests; it fails alone, at its own use, as a
//! recorded item failure while the other timers in that due batch proceed.
use super::super::{Statement, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, StatementBatch,
};
use std::sync::atomic::Ordering;
use workspace_model as m;
use workspace_model::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Op, CentralMailRequest,
    CentralMailResult as Result,
};

async fn execute(fixture: &Fixture, sql: &'static str, args: Vec<SqlValue>) {
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let outcome = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement { sql, args },
                expect: RowExpectation::Exactly(1),
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    assert!(matches!(outcome, BatchOutcome::Applied { .. }));
}

async fn request(fixture: &Fixture, number: u32, operation: Op) -> Result {
    fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(number),
                operation,
            },
        )
        .await
        .unwrap()
}

fn integer(value: i64) -> SqlValue {
    SqlValue::Integer(value.try_into().unwrap())
}

fn stamp(at: m::Timestamp) -> SqlValue {
    text(at.to_rfc3339())
}

/// The creator's notice window, opened by an observer joining the creator's thread.
async fn creator_window(fixture: &Fixture) -> SqlValue {
    let area = fixture.area(8490).await;
    let thread = fixture.thread(8491, area).await;
    let mut observer = fixture.caller.clone();
    let mut address = session(&observer);
    address.session_id = "row-use-observer".to_owned().try_into().unwrap();
    observer.caller = m::Caller::Session(address);
    let joined = call(
        &fixture.host,
        &observer,
        Some(fixture.workspace.clone()),
        8492,
        m::WorkspaceOperation::JoinThread {
            thread_id: thread,
            role: m::MemberRole::Observer,
        },
    )
    .await;
    assert!(matches!(
        joined.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let windows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT window_id FROM mail_notification_windows",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(windows.rows.len(), 1);
    windows.rows[0][0].clone()
}

#[derive(Clone, Copy)]
enum Corrupt {
    AutomationExpiry,
    AutomationAnchor,
    AutomationNextDue,
    WakeCreation,
    WindowDue,
}

#[tokio::test]
async fn invalid_timer_and_window_rows_fail_alone_at_their_own_use() {
    for (index, corrupt, column) in [
        (
            0_u32,
            Corrupt::AutomationExpiry,
            "automation_schedules.expires_at",
        ),
        (
            1,
            Corrupt::AutomationAnchor,
            "automation_schedules.anchor_at",
        ),
        (
            2,
            Corrupt::AutomationNextDue,
            "automation_schedules.next_due_at",
        ),
        (3, Corrupt::WakeCreation, "automation_wakes.created_at"),
        (4, Corrupt::WindowDue, "mail_notification_windows.due_at"),
    ] {
        let fixture = Fixture::new().await;
        let start = chrono::DateTime::from_timestamp_millis(fixture.clock.0.load(Ordering::SeqCst))
            .unwrap();
        // As in the stored-validation helper: admit one second past the mark, so
        // the interval's first occurrence is the next mark rather than now.
        fixture.clock.0.store(
            (start + chrono::Duration::seconds(1)).timestamp_millis(),
            Ordering::SeqCst,
        );
        let base = 8400 + index * 10;
        let Result::Done(Answer::AutomationSet {
            automation_id,
            next_due_at: Some(anchor),
            ..
        }) = request(
            &fixture,
            base,
            Op::SetAutomation {
                target: session(&fixture.caller),
                timing: serde_json::from_value(
                    serde_json::json!({"kind":"interval","seconds":600}),
                )
                .unwrap(),
                text: "row use automation".to_owned().try_into().unwrap(),
            },
        )
        .await
        else {
            panic!("automation refused")
        };
        let Result::Done(Answer::WakeSet { wake_id, .. }) = request(
            &fixture,
            base + 1,
            Op::SetWake {
                target: session(&fixture.caller),
                due_at: anchor,
                text: "row use wake".to_owned().try_into().unwrap(),
            },
        )
        .await
        else {
            panic!("wake refused")
        };
        let runtime = fixture.host.central(&fixture.caller.organization_id).await;
        // Arrange: store one invalid value and pick that row's own use, the due
        // batch that first loads it.
        let (use_at, kind, subject) = match corrupt {
            Corrupt::AutomationExpiry => {
                execute(
                    &fixture,
                    "UPDATE automation_schedules SET expires_at=? WHERE automation_id=?",
                    vec![
                        stamp(start + chrono::Duration::days(8)),
                        text(&automation_id),
                    ],
                )
                .await;
                (anchor, "automation_timer", text(&automation_id))
            }
            Corrupt::AutomationAnchor => {
                execute(
                    &fixture,
                    "UPDATE automation_schedules SET anchor_at=? WHERE automation_id=?",
                    vec![
                        stamp(anchor - chrono::Duration::seconds(300)),
                        text(&automation_id),
                    ],
                )
                .await;
                (anchor, "automation_timer", text(&automation_id))
            }
            Corrupt::AutomationNextDue => {
                // On the grid and within the horizon, but not a 600-second occurrence.
                let due = anchor + chrono::Duration::seconds(300);
                execute(
                    &fixture,
                    "UPDATE automation_schedules SET next_due_at=? WHERE automation_id=?",
                    vec![stamp(due), text(&automation_id)],
                )
                .await;
                (due, "automation_timer", text(&automation_id))
            }
            Corrupt::WakeCreation => {
                execute(
                    &fixture,
                    "UPDATE automation_wakes SET created_at='not-a-timestamp' WHERE wake_id=?",
                    vec![text(&wake_id)],
                )
                .await;
                (anchor, "wake_timer", text(&wake_id))
            }
            Corrupt::WindowDue => {
                let window = creator_window(&fixture).await;
                // A five-minute mark that is not on the default M15 schedule.
                assert_ne!(anchor.timestamp().rem_euclid(900), 0);
                execute(
                    &fixture,
                    "UPDATE mail_notification_windows SET due_at=? WHERE window_id=?",
                    vec![stamp(anchor), window],
                )
                .await;
                (anchor, "legacy_notice_windows", text("all"))
            }
        };
        fixture
            .clock
            .0
            .store(use_at.timestamp_millis(), Ordering::SeqCst);

        let work = runtime.after_event().await;

        let work = work.unwrap_or_else(|error| panic!("{column}: the event failed: {error:?}"));
        let records = runtime.statement_host().read(Statement { sql: "SELECT machine_kind,subject_key,state,failure_count,last_failure,last_failure_detail FROM engine_work_attempts", args: vec![] }).await.unwrap();
        assert_eq!(records.rows.len(), 1, "{column}: {:?}", records.rows);
        assert_eq!(
            &records.rows[0][..5],
            &[
                text(kind),
                subject,
                text("failing"),
                integer(1),
                text("corrupt_row")
            ],
            "{column}"
        );
        assert!(
            matches!(&records.rows[0][5], SqlValue::Text(detail) if detail.contains(column)),
            "{column}: {:?}",
            records.rows[0][5]
        );
        let effects = runtime.statement_host().read(Statement { sql: "SELECT (SELECT state FROM automation_wakes WHERE wake_id=?),(SELECT COUNT(*) FROM mail_messages WHERE message_kind='wake'),(SELECT COUNT(*) FROM mail_messages WHERE message_kind='automation'),(SELECT COUNT(*) FROM mail_messages WHERE message_kind='notice')", args: vec![text(&wake_id)] }).await.unwrap();
        let (wake_state, wakes, automations) = match corrupt {
            Corrupt::WakeCreation => ("active", 0, 1),
            Corrupt::WindowDue => ("done", 1, 1),
            _ => ("done", 1, 0),
        };
        assert_eq!(
            effects.rows,
            vec![vec![
                text(wake_state),
                integer(wakes),
                integer(automations),
                integer(0)
            ]],
            "{column}: the other timers proceed and the invalid row has no effect"
        );
        assert_eq!(
            (work.wakes_fired, work.automations_fired, work.windows_fired),
            (
                usize::try_from(wakes).unwrap(),
                usize::try_from(automations).unwrap(),
                0
            ),
            "{column}"
        );
        fixture.host.close().await;
    }
}
