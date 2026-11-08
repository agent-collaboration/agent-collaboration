//! Q4 at the automation store: the scheduler's next due time comes from the same rules its
//! steps act on, strictly after now, so an acted-on deadline never arms again.
use agent_automation::{
    ContinuityInput, ExecutionDestination, InstructionText, OperationId, RunId, ScheduleDefinition,
    TimingRule,
};
use automation_storage::{AutomationStore, ScheduleCreate};
use collaboration_protocol::{EndpointRef, SessionRef};
use serde_json::json;
use sqlx::Connection;

const BASE_MS: i64 = 1_800_000_000_000;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn the_next_due_time_is_the_earliest_actionable_item_strictly_after_now()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    let mut store = AutomationStore::open(&path).await?;
    let instruction = store
        .create_instruction(
            &OperationId::generate(),
            &InstructionText::try_from("Check job".to_owned())?,
            BASE_MS,
        )
        .await?;
    let target: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},
        "sessionId":"next-due"
    }))?;
    let schedule = store
        .create_schedule(&ScheduleCreate::<SessionRef, EndpointRef> {
            operation_id: OperationId::generate(),
            definition: ScheduleDefinition {
                instruction_id: instruction.instruction_id,
                timing: TimingRule::Interval { seconds: 3_600 },
                enabled: true,
                destination: ExecutionDestination::OwnedThread {
                    target,
                    cwd: "/tmp".into(),
                },
                execution_timeout_seconds: Some(120),
                model: Some("fixture-model".into()),
                effort: Some("medium".into()),
            },
            imported_continuity: ContinuityInput::None,
            now_ms: BASE_MS,
        })
        .await?;
    let mut seed = sqlx::SqliteConnection::connect(&format!("sqlite://{}", path.display())).await?;
    for (status, deadline_ms, summary) in [
        ("executing", Some(BASE_MS + 100_000), None),
        ("executing", Some(BASE_MS - 1_000), None),
        ("stopping", Some(BASE_MS + 5_000), None),
        (
            "summaryRunning",
            None,
            Some(json!({"phase":"running","deadlineAtMs":BASE_MS + 50_000})),
        ),
        (
            "summaryRunning",
            None,
            Some(json!({"phase":"stopping","deadlineAtMs":BASE_MS + 10_000})),
        ),
    ] {
        sqlx::query("INSERT INTO workflow_runs(run_id,schedule_id,due_at_ms,run_status,execution_evidence_json,execution_deadline_at_ms,summary_attempt_json) VALUES (?,?,?,?,'{}',?,?)")
            .bind(RunId::generate().as_str())
            .bind(schedule.schedule_id.as_str())
            .bind(BASE_MS)
            .bind(status)
            .bind(deadline_ms)
            .bind(summary.map(|attempt| attempt.to_string()))
            .execute(&mut seed)
            .await?;
    }
    seed.close().await?;

    let at_base = store.scheduler_next_due_at_ms(BASE_MS).await?;
    let at_summary_deadline = store.scheduler_next_due_at_ms(BASE_MS + 50_000).await?;
    let at_run_deadline = store.scheduler_next_due_at_ms(BASE_MS + 100_000).await?;
    let at_next_occurrence = store.scheduler_next_due_at_ms(BASE_MS + 3_600_000).await?;

    assert_eq!(
        at_base,
        Some(BASE_MS + 50_000),
        "a running summary's deadline; a stopping summary, a stopping Run and a past deadline are not due"
    );
    assert_eq!(at_summary_deadline, Some(BASE_MS + 100_000));
    assert_eq!(at_run_deadline, Some(BASE_MS + 3_600_000));
    assert_eq!(
        at_next_occurrence, None,
        "nothing is due after the last item"
    );
    store.close().await?;
    Ok(())
}
