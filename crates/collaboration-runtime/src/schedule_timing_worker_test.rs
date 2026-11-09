//! The scheduler uses one manual clock with real automation and lifecycle stores.
//! Causal task/store observations control progress; nextest bounds a stalled test.
use super::ScheduleTimingWorker;
use crate::scheduled_route_fixture::{
    FakeRouteKind, FakeScheduledExecution, FakeSettlementPlan, FakeSubmissionPlan, TestResult,
};
use crate::worker_test_support::{self, advance, settled_holding, wait_until};
use agent_automation::{
    AttemptId, ContinuityInput, ExecutionDestination, InstructionText, OperationId, RunPhase,
    ScheduleDefinition, TimingRule,
};
use automation_storage::{AutomationStore, ScheduleCreate};
use collaboration_contracts::{
    DeliveryContractError, DeliveryFuture, FreshSessionRequest, PreparationEvidenceSink,
    PreparedTarget, RunEvidenceSink, RunObservationContext, RunReconciliation, RunSettlement,
    RunSubmission, RunSummarySource, ScheduleDestination, SchedulePreparationOutcome,
    SchedulePreparationRequest, ScheduleSupport, ScheduledRunExecution, ScheduledRunSubmission,
    StopRequestOutcome,
};
use collaboration_protocol::{CodexGeneration, EndpointRef, SessionRef};
use router_clock::ManualClock;
use router_clock::RuntimeClock;
use runtime_primitives::{Clock, SyncTime};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const BASE_MS: i64 = 1_800_000_000_000;
const INTERVAL_SECONDS: u32 = 3_600;
const TIMEOUT_SECONDS: u32 = 120;

fn at(offset: Duration) -> SyncTime {
    SyncTime::from_unix_millis(BASE_MS).after(offset)
}

/// The fixture route, with a turn the test completes and the native reads it counts.
struct ControlledRoute {
    inner: FakeScheduledExecution,
    turn_completed: AtomicBool,
    settlement_reads: AtomicUsize,
    held_settlement: std::sync::Mutex<Option<Arc<tokio::sync::Notify>>>,
    stop_requests: std::sync::Mutex<Vec<SyncTime>>,
    clock: Arc<ManualClock>,
    initial_evidence_fails: AtomicBool,
    /// Preparation cannot reach the client: the step changes nothing and stays quiet.
    initial_evidence_unreachable: AtomicBool,
    /// When each preparation step asked for its route evidence.
    preparations: std::sync::Mutex<Vec<SyncTime>>,
    /// The Run each settlement read was for.
    observed_runs: std::sync::Mutex<Vec<agent_automation::RunId>>,
    /// How many target preparations still fail to reach the client, recording nothing.
    unreachable_targets: AtomicUsize,
    /// Fresh-session submissions, answered by the preparation tests.
    fresh: preparation_tests::FreshSubmissions,
    /// Per-Run answers and records of the capacity tests.
    per_run: capacity_tests::PerRunScript,
    /// How many settlement reads still answer that the client is unreachable.
    unreachable_settlements: AtomicUsize,
    /// How many stop requests still answer that the client is unreachable.
    unreachable_stops: AtomicUsize,
    /// A stop that reaches the client records its intent, as a real route does; otherwise
    /// the fixture answers that stopping is unsupported.
    stop_records_intent: AtomicBool,
    /// Stop requests that answer an unknown outcome: before storing the intent (the
    /// connection failed) or after it (the interrupt reply was lost).
    unknown_stops_before_intent: AtomicUsize,
    unknown_stops_after_intent: AtomicUsize,
}

/// Uses one of `remaining` unreachable answers, if any are left.
fn take_unreachable(remaining: &AtomicUsize) -> bool {
    remaining
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |left| {
            left.checked_sub(1)
        })
        .is_ok()
}

impl ScheduledRunExecution for ControlledRoute {
    fn support(&self, destination: &ScheduleDestination) -> DeliveryFuture<'_, ScheduleSupport> {
        self.inner.support(destination)
    }

    fn initial_evidence(
        &self,
        destination: &ScheduleDestination,
    ) -> DeliveryFuture<'_, agent_automation::RouteEffectEvidence<SessionRef, CodexGeneration>>
    {
        self.preparations.lock().unwrap().push(self.clock.now());
        if self.initial_evidence_unreachable.load(Ordering::Acquire) {
            return Box::pin(async { Err(DeliveryContractError::ClientOperation) });
        }
        if self.initial_evidence_fails.load(Ordering::Acquire) {
            return Box::pin(async { Err(DeliveryContractError::InvalidEvidence) });
        }
        self.inner.initial_evidence(destination)
    }

    fn prepare_destination<'a>(
        &'a self,
        request: SchedulePreparationRequest,
        sink: &'a dyn PreparationEvidenceSink,
    ) -> DeliveryFuture<'a, SchedulePreparationOutcome> {
        self.inner.prepare_destination(request, sink)
    }

    fn prepare_existing_target<'a>(
        &'a self,
        target: &SessionRef,
        declared_cwd: &str,
        sink: &'a dyn RunEvidenceSink,
    ) -> DeliveryFuture<'a, PreparedTarget> {
        if take_unreachable(&self.unreachable_targets) {
            return Box::pin(async { Err(DeliveryContractError::ClientOperation) });
        }
        self.inner
            .prepare_existing_target(target, declared_cwd, sink)
    }

    fn prepare_fresh_session<'a>(
        &'a self,
        request: FreshSessionRequest,
        sink: &'a dyn RunEvidenceSink,
    ) -> DeliveryFuture<'a, PreparedTarget> {
        self.inner.prepare_fresh_session(request, sink)
    }

    fn submit_run<'a>(
        &'a self,
        run: ScheduledRunSubmission,
        sink: &'a dyn RunEvidenceSink,
    ) -> DeliveryFuture<'a, RunSubmission> {
        if matches!(
            run.payload,
            collaboration_contracts::ScheduledRunPayload::Fresh { .. }
        ) {
            return self.fresh.submit(self.clock.now());
        }
        self.inner.submit_run(run, sink)
    }

    fn observe_settlement(
        &self,
        context: RunObservationContext,
    ) -> DeliveryFuture<'_, RunSettlement> {
        self.settlement_reads.fetch_add(1, Ordering::AcqRel);
        let settlement =
            (self.per_run).settlement(&context, self.turn_completed.load(Ordering::Acquire));
        self.observed_runs.lock().unwrap().push(context.run_id);
        let unreachable = take_unreachable(&self.unreachable_settlements);
        let held = self.held_settlement.lock().unwrap().clone();
        Box::pin(async move {
            if let Some(held) = held {
                held.notified().await;
            }
            if unreachable {
                return Err(DeliveryContractError::ClientOperation);
            }
            Ok(settlement)
        })
    }

    fn summary_source(
        &self,
        context: RunObservationContext,
    ) -> DeliveryFuture<'_, RunSummarySource> {
        self.inner.summary_source(context)
    }

    fn request_stop<'a>(
        &'a self,
        context: RunObservationContext,
        sink: &'a dyn RunEvidenceSink,
    ) -> DeliveryFuture<'a, StopRequestOutcome> {
        self.stop_requests.lock().unwrap().push(self.clock.now());
        self.per_run.stop_requested(&context.run_id);
        if take_unreachable(&self.unreachable_stops) {
            return Box::pin(async { Err(DeliveryContractError::ClientOperation) });
        }
        if take_unreachable(&self.unknown_stops_before_intent) {
            return Box::pin(async { Ok(StopRequestOutcome::Unknown) });
        }
        if take_unreachable(&self.unknown_stops_after_intent) {
            return Box::pin(async move {
                sink.record_stop_intent().await?;
                Ok(StopRequestOutcome::Unknown)
            });
        }
        if !self.stop_records_intent.load(Ordering::Acquire) {
            return self.inner.request_stop(context, sink);
        }
        Box::pin(async move {
            sink.record_stop_intent().await?;
            Ok(StopRequestOutcome::Requested)
        })
    }

    fn reconcile_run(
        &self,
        context: RunObservationContext,
    ) -> DeliveryFuture<'_, RunReconciliation> {
        self.inner.reconcile_run(context)
    }
}

struct SchedulerFixture {
    root: tempfile::TempDir,
    store: Arc<Mutex<AutomationStore>>,
    interactions: Arc<interaction_storage::InteractionStore>,
    journal: Arc<lifecycle_observation::LifecycleStore>,
    route: Arc<ControlledRoute>,
    clock: Arc<ManualClock>,
}

impl SchedulerFixture {
    /// One enabled schedule, first due at base + one interval, on the fixture route.
    async fn with_one_schedule() -> TestResult<Self> {
        let fixture = Self::empty().await?;
        fixture.create_schedule().await?;
        Ok(fixture)
    }

    /// A real automation store with no schedules and no Runs.
    async fn empty() -> TestResult<Self> {
        let root = tempfile::tempdir()?;
        let store = Arc::new(Mutex::new(
            AutomationStore::open(&root.path().join("automation.sqlite")).await?,
        ));
        let interactions = Arc::new(
            interaction_storage::InteractionStore::open(root.path().join("interaction.sqlite"))
                .await?,
        );
        let journal = Arc::new(lifecycle_observation::LifecycleStore::new(
            lifecycle_observation::ObservationJournal::open(
                &root.path().join("session-registry.sqlite"),
                "00000000-0000-4000-8000-000000000001"
                    .to_owned()
                    .try_into()?,
            )
            .await?,
        ));
        journal.prepare(BASE_MS / 1_000).await?;
        let service = "00000000-0000-4000-8000-000000000001";
        let target: SessionRef = serde_json::from_value(json!({
            "endpoint":{"serviceId":service,"endpointId":"claude-local"},
            "sessionId":"scheduled-worker"
        }))?;
        let clock = Arc::new(ManualClock::starting_at(at(Duration::ZERO)).unwrap());
        let route = Arc::new(ControlledRoute {
            inner: FakeScheduledExecution {
                kind: FakeRouteKind::Provider,
                target,
                interactions: Arc::clone(&interactions),
                observed_push_ids: Arc::default(),
                submit_calls: Arc::default(),
                generation: serde_json::from_value(json!({"serviceEpoch":service,"generation":1}))?,
                attempt_id: AttemptId::generate(),
                submissions: AtomicUsize::new(0),
                submission_plan: FakeSubmissionPlan::Accept,
                settlement_plan: FakeSettlementPlan::Pending,
            },
            turn_completed: AtomicBool::new(false),
            settlement_reads: AtomicUsize::new(0),
            held_settlement: std::sync::Mutex::default(),
            stop_requests: std::sync::Mutex::default(),
            clock: Arc::clone(&clock),
            initial_evidence_fails: AtomicBool::new(false),
            initial_evidence_unreachable: AtomicBool::new(false),
            preparations: std::sync::Mutex::default(),
            observed_runs: std::sync::Mutex::default(),
            unreachable_targets: AtomicUsize::new(0),
            fresh: preparation_tests::FreshSubmissions::default(),
            per_run: capacity_tests::PerRunScript::default(),
            unreachable_settlements: AtomicUsize::new(0),
            unreachable_stops: AtomicUsize::new(0),
            stop_records_intent: AtomicBool::new(false),
            unknown_stops_before_intent: AtomicUsize::new(0),
            unknown_stops_after_intent: AtomicUsize::new(0),
        });
        Ok(Self {
            root,
            store,
            interactions,
            journal,
            route,
            clock,
        })
    }

    async fn create_schedule(&self) -> TestResult<()> {
        let instruction = self
            .store
            .lock()
            .await
            .create_instruction(
                &OperationId::generate(),
                &InstructionText::try_from("Check job".to_owned())?,
                BASE_MS,
            )
            .await?;
        self.store
            .lock()
            .await
            .create_schedule(&ScheduleCreate::<SessionRef, EndpointRef> {
                operation_id: OperationId::generate(),
                definition: ScheduleDefinition {
                    instruction_id: instruction.instruction_id,
                    timing: TimingRule::Interval {
                        seconds: INTERVAL_SECONDS,
                    },
                    enabled: true,
                    destination: ExecutionDestination::OwnedThread {
                        target: self.route.inner.target.clone(),
                        cwd: self.root.path().to_string_lossy().into_owned(),
                    },
                    execution_timeout_seconds: Some(TIMEOUT_SECONDS),
                    model: Some("fixture-model".into()),
                    effort: Some("medium".into()),
                },
                imported_continuity: ContinuityInput::None,
                now_ms: BASE_MS,
            })
            .await?;
        Ok(())
    }

    fn start(&self) -> TestResult<RunningScheduler> {
        self.start_with(|worker| worker)
    }

    /// Starts the scheduler with every step task held after it reported, until `gate`
    /// is released.
    fn start_holding_step_exits(&self, gate: Arc<Mutex<()>>) -> TestResult<RunningScheduler> {
        self.start_with(|worker| worker.hold_step_exits(gate))
    }

    fn start_with(
        &self,
        configure: impl FnOnce(ScheduleTimingWorker) -> ScheduleTimingWorker,
    ) -> TestResult<RunningScheduler> {
        self.start_on(None, configure)
    }

    /// Starts the scheduler with a native control backend, which summary steps use.
    fn start_on(
        &self,
        backend: Option<crate::NativeControlBackend>,
        configure: impl FnOnce(ScheduleTimingWorker) -> ScheduleTimingWorker,
    ) -> TestResult<RunningScheduler> {
        let worker = ScheduleTimingWorker::new(
            Arc::clone(&self.store),
            Arc::clone(&self.interactions),
            Arc::clone(&self.route) as Arc<dyn ScheduledRunExecution>,
            backend,
            crate::AutomationConfigurationHandle::default(),
            collaboration_contracts::MachineIdentity::new(
                "00000000-0000-4000-8000-000000000001"
                    .to_owned()
                    .try_into()?,
                Some("Schedule Fixture"),
            )?,
            Arc::clone(&self.clock) as Arc<dyn router_clock::RuntimeClock>,
        )
        .wake_on_completions(self.journal.subscribe_commits());
        let worker = configure(worker);
        let observation = worker.observation();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(worker.run(shutdown.clone()));
        Ok(RunningScheduler {
            observation,
            shutdown,
            task,
        })
    }

    async fn run_phase(&self) -> TestResult<Option<(agent_automation::RunId, RunPhase)>> {
        let ids = self.store.lock().await.observable_run_ids(None).await?;
        let Some(id) = ids.first() else {
            return Ok(None);
        };
        let record = self
            .store
            .lock()
            .await
            .read_run::<SessionRef, EndpointRef, CodexGeneration, crate::stored_run_receipt::StoredRunReceipt>(id)
            .await?;
        Ok(Some((id.clone(), record.phase)))
    }

    async fn read_phase(&self, id: &agent_automation::RunId) -> TestResult<RunPhase> {
        Ok(self
            .store
            .lock()
            .await
            .read_run::<SessionRef, EndpointRef, CodexGeneration, crate::stored_run_receipt::StoredRunReceipt>(id)
            .await?
            .phase)
    }

    /// Yields until the scheduler is asleep with a Run executing and checked at least once.
    /// It applies owed push operations as the push hand-off worker does; their
    /// acknowledgement is the automation commit that lets preparation continue. The fixture
    /// route answers its first submission "busy", which writes nothing; the target going
    /// idle reaches the scheduler as a journal commit, so the helper appends that once.
    async fn executing_and_asleep(
        &self,
        scheduler: &RunningScheduler,
    ) -> TestResult<agent_automation::RunId> {
        let mut handoff =
            crate::PushHandoffWorker::new(Arc::clone(&self.store), Arc::clone(&self.interactions));
        let mut target_went_idle = false;
        loop {
            handoff.apply_pending().await?;
            if !target_went_idle
                && self.route.inner.submissions.load(Ordering::Acquire) == 1
                && settle(scheduler).await
            {
                let idle = thread_observation(
                    json!({"kind":"threadStatus","status":{"type":"idle"},"ordering":"established"}),
                )?;
                self.journal.append(&idle, BASE_MS / 1_000).await?;
                target_went_idle = true;
            }
            if let Some((id, RunPhase::Executing)) = self.run_phase().await?
                && self.route.settlement_reads.load(Ordering::Acquire) > 0
                && settle(scheduler).await
            {
                return Ok(id);
            }
            tokio::task::yield_now().await;
        }
    }
}

/// A native observation of the fixture thread, as the observer would record it.
fn thread_observation(
    change: serde_json::Value,
) -> TestResult<collaboration_protocol::LifecycleObservation> {
    let endpoint =
        json!({"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"});
    Ok(serde_json::from_value(json!({
        "observedAt":"2027-01-15T08:01:00.000Z","source":"nativeNotification",
        "scope":{"endpoint":endpoint,
            "generation":{"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1},
            "observerId":"00000000-0000-4000-8000-000000000003"},
        "subject":{"kind":"thread","address":{"endpoint":endpoint,"nativeThreadId":"scheduled-worker"}},
        "change":change,
    }))?)
}

struct RunningScheduler {
    observation: crate::WorkerObservation,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl RunningScheduler {
    async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
    }
}

/// Waits, without moving the clock, until the scheduler is asleep and stays so.
async fn settled(scheduler: &RunningScheduler) -> TestResult<()> {
    worker_test_support::settled(&scheduler.observation).await
}

async fn settle(scheduler: &RunningScheduler) -> bool {
    worker_test_support::settle(&scheduler.observation).await
}

#[path = "schedule_timing_worker_ordering_test.rs"]
mod ordering_tests;

#[path = "schedule_timing_worker_capacity_test.rs"]
mod capacity_tests;

#[path = "schedule_timing_worker_preparation_test.rs"]
mod preparation_tests;

#[path = "schedule_timing_worker_summary_test.rs"]
mod summary_tests;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn an_idle_scheduler_reads_nothing_until_its_next_schedule_is_due() -> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let asleep = scheduler.observation.snapshot();

    advance(
        &fixture.clock,
        Duration::from_secs(u64::from(INTERVAL_SECONDS) - 1),
    )
    .await;

    assert_eq!(
        asleep.reads,
        [
            "due schedules",
            "waiting schedules",
            "observable runs",
            "scheduler next due"
        ]
    );
    assert_eq!(
        asleep.armed_deadline,
        Some(at(Duration::from_secs(u64::from(INTERVAL_SECONDS)))),
        "one timer, at the schedule's next occurrence"
    );
    assert_eq!(
        scheduler.observation.snapshot(),
        asleep,
        "no read before the due time"
    );
    scheduler.stop().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_due_schedule_runs_at_its_time_and_its_run_deadline_stops_it_once() -> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let interval = Duration::from_secs(u64::from(INTERVAL_SECONDS));
    let deadline = interval + Duration::from_secs(u64::from(TIMEOUT_SECONDS));

    advance(&fixture.clock, interval).await;
    let run = fixture.executing_and_asleep(&scheduler).await?;
    let armed_for_deadline = scheduler.observation.snapshot().armed_deadline;
    advance(
        &fixture.clock,
        Duration::from_secs(u64::from(TIMEOUT_SECONDS) - 1),
    )
    .await;
    let stops_before_deadline = fixture.route.stop_requests.lock().unwrap().len();
    advance(&fixture.clock, Duration::from_secs(1)).await;
    settled(&scheduler).await?;
    let armed_after_deadline = scheduler.observation.snapshot().armed_deadline;
    advance(&fixture.clock, Duration::from_secs(600)).await;

    assert_eq!(fixture.read_phase(&run).await?, RunPhase::Executing);
    assert_eq!(
        armed_for_deadline,
        Some(at(deadline)),
        "the run deadline arms the timer"
    );
    assert_eq!(
        stops_before_deadline, 0,
        "nothing stops the run before its deadline"
    );
    assert_eq!(
        *fixture.route.stop_requests.lock().unwrap(),
        [at(deadline)],
        "the deadline acts once, at its time"
    );
    assert_eq!(
        armed_after_deadline,
        Some(at(interval * 2)),
        "an acted-on deadline does not arm again; the next occurrence does"
    );
    scheduler.stop().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_running_turn_is_checked_once_and_its_completion_settles_the_run_with_no_clock_advance()
-> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    advance(
        &fixture.clock,
        Duration::from_secs(u64::from(INTERVAL_SECONDS)),
    )
    .await;
    let run = fixture.executing_and_asleep(&scheduler).await?;
    let checks_while_running = fixture.route.settlement_reads.load(Ordering::Acquire);

    advance(&fixture.clock, Duration::from_secs(60)).await;
    let checks_after_a_quiet_minute = fixture.route.settlement_reads.load(Ordering::Acquire);
    fixture.route.turn_completed.store(true, Ordering::Release);
    let completed_at = fixture.clock.monotonic_now();
    let turn_completed = thread_observation(
        json!({"kind":"turnTerminal","turnId":"fixture-turn","status":"completed"}),
    )?;
    fixture
        .journal
        .append(&turn_completed, BASE_MS / 1_000)
        .await?;
    while fixture.read_phase(&run).await? == RunPhase::Executing {
        tokio::task::yield_now().await;
    }

    assert_eq!(
        checks_after_a_quiet_minute, checks_while_running,
        "a running turn is not re-checked"
    );
    assert_eq!(fixture.read_phase(&run).await?, RunPhase::Finished);
    assert_eq!(
        fixture.clock.monotonic_now(),
        completed_at,
        "completion needs no clock advance"
    );
    assert_eq!(
        fixture.route.settlement_reads.load(Ordering::Acquire),
        checks_while_running + 1,
        "the completion causes exactly one check"
    );
    assert_eq!(
        fixture.route.inner.submissions.load(Ordering::Acquire),
        2,
        "the busy answer and the accepted one, and no other"
    );
    scheduler.stop().await;
    Ok(())
}

async fn observe(fixture: &SchedulerFixture, change: serde_json::Value) -> TestResult<()> {
    fixture
        .journal
        .append(&thread_observation(change)?, BASE_MS / 1_000)
        .await?;
    Ok(())
}

fn turn_completed() -> serde_json::Value {
    json!({"kind":"turnTerminal","turnId":"fixture-turn","status":"completed"})
}

/// The scheduler's own per-Run failed-step warnings among the captured ones.
fn step_warnings(warnings: &crate::captured_warnings_test_support::CapturedWarnings) -> usize {
    warnings
        .events()
        .iter()
        .filter(|warning| warning.message.contains("scheduled Run step unavailable"))
        .count()
}

#[path = "schedule_timing_worker_retry_test.rs"]
mod retry_tests;
