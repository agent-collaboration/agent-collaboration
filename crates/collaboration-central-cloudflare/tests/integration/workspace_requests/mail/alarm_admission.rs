//! Persisted timer history and admission policy must not gate unrelated mail.
use super::super::{Statement, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, ReadOutcome, RowExpectation, SqlValue,
    StatementBatch,
};
use collaboration_central_conformance::ConformanceResult as TestResult;
use std::sync::atomic::Ordering;
use workspace_model as m;
use workspace_model::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Op, CentralMailRequest,
    CentralMailResult as MailResult,
};

fn now(f: &Fixture) -> TestResult<m::Timestamp> {
    m::Timestamp::from_timestamp_millis(f.clock.0.load(Ordering::SeqCst))
        .ok_or_else(|| "fixture clock is invalid".into())
}
fn set_time(f: &Fixture, at: m::Timestamp) {
    f.clock.0.store(at.timestamp_millis(), Ordering::SeqCst);
}
fn number(value: i64) -> TestResult<SqlValue> {
    Ok(SqlValue::Integer(value.try_into()?))
}
async fn rows(f: &Fixture, sql: &'static str, args: Vec<SqlValue>) -> TestResult<ReadOutcome> {
    let runtime = f.host.central(&f.caller.organization_id).await;
    Ok(runtime
        .statement_host()
        .read(Statement { sql, args })
        .await?)
}
async fn batch(
    f: &Fixture,
    statements: Vec<GuardedStatement>,
    alarm: AlarmUpdate,
) -> TestResult<()> {
    let runtime = f.host.central(&f.caller.organization_id).await;
    if !matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch { statements, alarm })
            .await?,
        BatchOutcome::Applied { .. }
    ) {
        return Err("fixture owning batch did not apply".into());
    }
    Ok(())
}
async fn execute(f: &Fixture, sql: &'static str, args: Vec<SqlValue>) -> TestResult<()> {
    batch(
        f,
        vec![GuardedStatement {
            statement: Statement { sql, args },
            expect: RowExpectation::Exactly(1),
        }],
        AlarmUpdate::Unchanged,
    )
    .await
}
async fn alarm(f: &Fixture) -> TestResult<Option<i64>> {
    let runtime = f.host.central(&f.caller.organization_id).await;
    Ok(runtime
        .control::<Option<i64>>("raw-alarm-time", &())
        .await?)
}
async fn request(f: &Fixture, serial: u32, operation: Op) -> TestResult<MailResult> {
    Ok(f.host
        .request_central(
            &f.caller,
            CentralMailRequest {
                request_id: id(serial),
                operation,
            },
        )
        .await?)
}
async fn set_wake(
    f: &Fixture,
    serial: u32,
    due: m::Timestamp,
) -> TestResult<workspace_ids::WakeId> {
    match request(
        f,
        serial,
        Op::SetWake {
            target: session(&f.caller),
            due_at: due,
            text: "retained wake".to_owned().try_into()?,
        },
    )
    .await?
    {
        MailResult::Done(Answer::WakeSet { wake_id, due_at }) if due_at == due => Ok(wake_id),
        _ => Err("fixture wake was not admitted".into()),
    }
}
async fn send(f: &Fixture, serial: u32) -> TestResult<MailResult> {
    request(
        f,
        serial,
        Op::SendMessage {
            to: session(&f.caller),
            body: "unrelated fresh mail".to_owned().try_into()?,
        },
    )
    .await
}
async fn corrupt_done_history(f: &Fixture, serial: u32) -> TestResult<workspace_ids::WakeId> {
    let wake = set_wake(f, serial, now(f)? + chrono::Duration::seconds(300)).await?;
    // Direct SQL is legacy/corrupt-history setup, never an admitted product write.
    execute(f, "UPDATE wakes SET state='done',target='not-json',created_at='not-a-timestamp' WHERE wake_id=?", vec![text(&wake)]).await?;
    Ok(wake)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A corrupt terminal history row must leave unrelated mail and alarm repair usable."
)]
#[tokio::test]
async fn corrupt_done_wake_does_not_block_fresh_mail_or_alarm_recomputation() -> TestResult<()> {
    let f = Fixture::new().await;
    let due = now(&f)? + chrono::Duration::seconds(600);
    set_wake(&f, 8400, due).await?;
    let history = corrupt_done_history(&f, 8401).await?;
    let before = rows(
        &f,
        "SELECT * FROM wakes WHERE wake_id=?",
        vec![text(&history)],
    )
    .await?
    .rows;
    // The existing Worker central-batch control uses AlarmPolicy::Ignore, so a
    // raw SetFromQuery(NULL) cannot clear the platform alarm. The prior wake's
    // deadline may remain armed after raw history setup. The real mail request
    // below must succeed and replace it with the healthy active deadline.
    assert!(
        matches!(
            send(&f, 8402).await?,
            MailResult::Done(Answer::Queued { .. })
        ),
        "terminal history must not refuse an unrelated admitted mail write"
    );
    assert_eq!(rows(&f,"SELECT (SELECT COUNT(*) FROM messages WHERE message_kind='direct'),(SELECT COUNT(*) FROM request_log WHERE request_id=?)",vec![text(id::<m::RequestId>(8402))]).await?.rows,vec![vec![number(1)?,number(1)?]]);
    assert_eq!(
        alarm(&f).await?,
        Some(due.timestamp_millis()),
        "mail must repair the actual alarm from healthy active rows"
    );
    assert!(
        rows(
            &f,
            "SELECT * FROM wakes WHERE wake_id=?",
            vec![text(history)]
        )
        .await?
        .rows
            == before,
        "unrelated mail must not rewrite historical facts"
    );
    f.host.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Cold runtime recomputation must ignore corrupt terminal history and preserve a healthy future alarm."
)]
#[tokio::test]
async fn corrupt_done_wake_does_not_prevent_cold_runtime_startup() -> TestResult<()> {
    let f = Fixture::new().await;
    let due = now(&f)? + chrono::Duration::seconds(600);
    set_wake(&f, 8410, due).await?;
    let history = corrupt_done_history(&f, 8411).await?;
    let before = rows(
        &f,
        "SELECT * FROM wakes WHERE wake_id=?",
        vec![text(&history)],
    )
    .await?
    .rows;
    let runtime = f.host.cold_runtime(&f.caller.organization_id);
    assert_eq!(runtime.cold_alarm().await?.wakes_fired, 0);
    // The actual workerd cold alarm initializes the runtime and recomputes due state.
    assert_eq!(alarm(&f).await?, Some(due.timestamp_millis()));
    assert!(
        matches!(
            send(&f, 8412).await?,
            MailResult::Done(Answer::Queued { .. })
        ),
        "cold runtime must accept fresh unrelated mail"
    );
    assert!(
        rows(
            &f,
            "SELECT * FROM wakes WHERE wake_id=?",
            vec![text(history)]
        )
        .await?
        .rows
            == before
    );
    f.host.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Malformed active due text must not roll back unrelated durable mail or suppress the healthy physical alarm."
)]
#[tokio::test]
async fn malformed_active_due_does_not_rollback_mail_or_healthy_alarm_rearm() -> TestResult<()> {
    let f = Fixture::new().await;
    let due = now(&f)? + chrono::Duration::seconds(600);
    set_wake(&f, 8420, due).await?;
    let malformed = set_wake(&f, 8421, due + chrono::Duration::seconds(300)).await?;
    execute(
        &f,
        "UPDATE wakes SET due_at='not-a-timestamp' WHERE wake_id=?",
        vec![text(malformed)],
    )
    .await?;
    // Timing allowance: setup claimed a due batch at T0 and the unreadable due
    // makes this send's event the first due batch, so hold the clock at T0 + 30 s.
    set_time(&f, due - chrono::Duration::seconds(570));
    assert!(
        matches!(
            send(&f, 8422).await?,
            MailResult::Done(Answer::Queued { .. })
        ),
        "one invalid active due must not roll back the unrelated request"
    );
    assert_eq!(rows(&f,"SELECT (SELECT COUNT(*) FROM messages WHERE message_kind='direct'),(SELECT COUNT(*) FROM request_log WHERE request_id=?)",vec![text(id::<m::RequestId>(8422))]).await?.rows,vec![vec![number(1)?,number(1)?]]);
    // Disposition: the unreadable active due counts as due now, so the healthy
    // re-arm lands on the next five-minute mark rather than the healthy due.
    let next_mark = (now(&f)?.timestamp_millis().div_euclid(300_000) + 1) * 300_000;
    assert_eq!(
        alarm(&f).await?,
        Some(next_mark),
        "the unreadable active due must re-arm at the next five-minute mark"
    );
    f.host.close().await;
    Ok(())
}

// shaped_legacy_wake_fires_despite_current_grid_or_horizon_policy is Deferred to
// deliverable 2 (Lead ruling): stored wake-time validation is kept.
