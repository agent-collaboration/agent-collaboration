//! Reusable real-store worker fixture, with a literal clock for schedule history.
use super::{
    Arc, AtomicUsize, AttemptId, AutomationStore, CodexGeneration, ContinuityInput, EndpointRef,
    ExecutionDestination, FakeRouteKind, FakeScheduledExecution, FakeSettlementPlan,
    FakeSubmissionPlan, InstructionText, Mutex, OperationId, PushId, RunId, ScheduleCreate,
    ScheduleDefinition, ScheduledRunExecution, ScheduledRunWorker, SessionRef, TestResult,
    TimingRule, json,
};

#[derive(Clone, Copy)]
pub struct FixtureTiming {
    pub anchor_ms: i64,
    pub interval_seconds: u32,
}
impl FixtureTiming {
    fn first_due_ms(self) -> i64 {
        self.anchor_ms + i64::from(self.interval_seconds) * 1000
    }
}

pub async fn apply_push_handoff(worker: &ScheduledRunWorker) -> TestResult<()> {
    crate::PushHandoffWorker::new(Arc::clone(&worker.store), Arc::clone(&worker.interactions))
        .apply_pending()
        .await?;
    Ok(())
}

pub async fn provider_worker_fixture(
    submission_plan: FakeSubmissionPlan,
    settlement_plan: FakeSettlementPlan,
) -> TestResult<(
    std::path::PathBuf,
    Arc<Mutex<AutomationStore>>,
    ScheduledRunWorker,
    RunId,
    Arc<std::sync::Mutex<Vec<PushId>>>,
)> {
    provider_worker_fixture_at(
        submission_plan,
        settlement_plan,
        FixtureTiming {
            anchor_ms: 0,
            interval_seconds: 60,
        },
    )
    .await
}

pub async fn provider_worker_fixture_at(
    submission_plan: FakeSubmissionPlan,
    settlement_plan: FakeSettlementPlan,
    timing: FixtureTiming,
) -> TestResult<(
    std::path::PathBuf,
    Arc<Mutex<AutomationStore>>,
    ScheduledRunWorker,
    RunId,
    Arc<std::sync::Mutex<Vec<PushId>>>,
)> {
    let root = std::env::temp_dir().join(format!(
        "provider-worker-{}",
        OperationId::generate().as_str()
    ));
    std::fs::create_dir(&root)?;
    let store = Arc::new(Mutex::new(
        AutomationStore::open(&root.join("automation.sqlite")).await?,
    ));
    let service = "00000000-0000-4000-8000-000000000001";
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(root.join("interaction.sqlite")).await?,
    );
    let target: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":service,"endpointId":"claude-local"},
        "sessionId":"provider-worker"
    }))?;
    let generation: CodexGeneration =
        serde_json::from_value(json!({"serviceEpoch":service,"generation":1}))?;
    let observed_push_ids = Arc::new(std::sync::Mutex::new(Vec::new()));
    let fake: Arc<dyn ScheduledRunExecution> = Arc::new(FakeScheduledExecution {
        kind: FakeRouteKind::Provider,
        target: target.clone(),
        interactions: Arc::clone(&interactions),
        observed_push_ids: Arc::clone(&observed_push_ids),
        generation,
        attempt_id: AttemptId::generate(),
        submissions: AtomicUsize::new(0),
        submission_plan,
        settlement_plan,
    });
    let instruction = store
        .lock()
        .await
        .create_instruction(
            &OperationId::generate(),
            &InstructionText::try_from("Check job".to_owned())?,
            timing.anchor_ms,
        )
        .await?;
    let schedule = store
        .lock()
        .await
        .create_schedule(&ScheduleCreate::<SessionRef, EndpointRef> {
            operation_id: OperationId::generate(),
            definition: ScheduleDefinition {
                instruction_id: instruction.instruction_id,
                timing: TimingRule::Interval {
                    seconds: timing.interval_seconds,
                },
                enabled: true,
                destination: ExecutionDestination::OwnedThread {
                    target,
                    cwd: root.to_string_lossy().into_owned(),
                },
                execution_timeout_seconds: Some(120),
                model: Some("fixture-model".into()),
                effort: Some("medium".into()),
            },
            imported_continuity: ContinuityInput::None,
            now_ms: timing.anchor_ms,
        })
        .await?;
    let run_id = store
        .lock()
        .await
        .enqueue_due_run::<SessionRef, EndpointRef>(&schedule.schedule_id, timing.first_due_ms())
        .await?
        .ok_or("run missing")?;
    store
        .lock()
        .await
        .admit_waiting_run::<SessionRef, EndpointRef>(&schedule.schedule_id, timing.first_due_ms())
        .await?;
    let worker = ScheduledRunWorker {
        store: Arc::clone(&store),
        interactions,
        execution: fake,
        backend: None,
        configuration: crate::AutomationConfigurationHandle::default(),
        machine_identity: collaboration_contracts::MachineIdentity::new(
            service.to_owned().try_into()?,
            Some("Schedule Fixture"),
        )?,
    };
    Ok((root, store, worker, run_id, observed_push_ids))
}
