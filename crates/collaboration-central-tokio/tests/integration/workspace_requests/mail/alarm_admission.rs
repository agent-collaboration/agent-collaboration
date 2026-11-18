//! Persisted timer history and admission policy must not gate unrelated mail.
use super::super::{Statement, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, ReadOutcome, RowExpectation, SqlValue,
    StatementBatch,
};
use collaboration_central_conformance::ConformanceResult as TestResult;
use router_clock::RuntimeClock;
use workspace_model as m;
use workspace_model::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Op, CentralMailRequest,
    CentralMailResult as MailResult,
};

fn now(f: &Fixture) -> TestResult<m::Timestamp> {
    Ok(f.clock.utc_now())
}
async fn set_time(f: &Fixture, at: m::Timestamp) {
    f.set_utc(at).await;
}
fn number(value: i64) -> TestResult<SqlValue> {
    Ok(SqlValue::Integer(value.try_into()?))
}
async fn rows(f: &Fixture, sql: &'static str, args: Vec<SqlValue>) -> TestResult<ReadOutcome> {
    let runtime = f.host.central(f.caller.organization_id()).await?;
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
    let runtime = f.host.central(f.caller.organization_id()).await?;
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
async fn clear_alarm(f: &Fixture) -> TestResult<()> {
    // Simulate interrupted publication using the established storage alarm seam.
    batch(
        f,
        vec![],
        AlarmUpdate::SetFromQuery(Statement {
            sql: "SELECT NULL AS earliest_due",
            args: vec![],
        }),
    )
    .await
}
async fn alarm(f: &Fixture) -> TestResult<Option<i64>> {
    let runtime = f.host.central(f.caller.organization_id()).await?;
    Ok(runtime.statement_host().alarm().map(|at| at.unix_millis()))
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
            delivery: workspace_model::central_mail::MessageDelivery::Auto,
            reply_to: None,
            sent_at: "2100-01-01T00:00:00Z".parse().unwrap(),
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
    clear_alarm(&f).await?;
    assert_eq!(alarm(&f).await?, None);
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
    let mut f = Fixture::new().await;
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
    clear_alarm(&f).await?;
    f.reopen().await;
    let runtime = f.host.central(f.caller.organization_id()).await?;
    runtime.recompute_alarm().await?;
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
    clear_alarm(&f).await?;
    // Timing allowance: setup claimed a due batch at T0 and the unreadable due
    // makes this send's event the first due batch, so hold the clock at T0 + 30 s.
    set_time(&f, due - chrono::Duration::seconds(570)).await;
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
