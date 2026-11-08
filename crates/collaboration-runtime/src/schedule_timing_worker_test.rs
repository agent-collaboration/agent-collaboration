//! The scheduler on Tokio's paused clock, with a real automation store, a real lifecycle
//! journal as its completion source and a route whose turn the test completes.
//!
//! Steps wait on SQLite's worker thread, so a paused runtime that looks idle would jump to
//! its next timer. Each fixture holds a blocking task, which stops Tokio's auto-advance, so
//! time moves only when a test advances it.
use super::ScheduleTimingWorker;
use crate::{
    paused_wall_clock::PausedWallClock,
    scheduled_route_fixture::{
        FakeRouteKind, FakeScheduledExecution, FakeSettlementPlan, FakeSubmissionPlan, TestResult,
    },
};
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
    stop_requests: std::sync::Mutex<Vec<SyncTime>>,
    clock: Arc<PausedWallClock>,
    initial_evidence_fails: AtomicBool,
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
        self.inner.submit_run(run, sink)
    }

    fn observe_settlement(&self, _: RunObservationContext) -> DeliveryFuture<'_, RunSettlement> {
        self.settlement_reads.fetch_add(1, Ordering::AcqRel);
        let completed = self.turn_completed.load(Ordering::Acquire);
        Box::pin(async move {
            Ok(if completed {
                RunSettlement::Completed {
                    summary_source: None,
                }
            } else {
                RunSettlement::Pending
            })
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
        self.inner.request_stop(context, sink)
    }

    fn reconcile_run(
        &self,
        context: RunObservationContext,
    ) -> DeliveryFuture<'_, RunReconciliation> {
        self.inner.reconcile_run(context)
    }
}

/// While this blocking task runs, Tokio's paused clock does not auto-advance.
struct NoAutoAdvance {
    release: Option<std::sync::mpsc::Sender<()>>,
}

impl NoAutoAdvance {
    fn hold() -> Self {
        let (release, released) = std::sync::mpsc::channel::<()>();
        drop(tokio::task::spawn_blocking(move || {
            let _ = released.recv();
        }));
        Self {
            release: Some(release),
        }
    }
}

impl Drop for NoAutoAdvance {
    fn drop(&mut self) {
        drop(self.release.take());
    }
}

struct SchedulerFixture {
    _no_auto_advance: NoAutoAdvance,
    root: tempfile::TempDir,
    store: Arc<Mutex<AutomationStore>>,
    interactions: Arc<interaction_storage::InteractionStore>,
    journal: Arc<lifecycle_observation::LifecycleStore>,
    route: Arc<ControlledRoute>,
    clock: Arc<PausedWallClock>,
}

impl SchedulerFixture {
    /// One enabled schedule, first due at base + one interval, on the fixture route.
    async fn with_one_schedule() -> TestResult<Self> {
        let no_auto_advance = NoAutoAdvance::hold();
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
        let clock = Arc::new(PausedWallClock::starting_at(at(Duration::ZERO)));
        let route = Arc::new(ControlledRoute {
            inner: FakeScheduledExecution {
                kind: FakeRouteKind::Provider,
                target: target.clone(),
                interactions: Arc::clone(&interactions),
                observed_push_ids: Arc::default(),
                generation: serde_json::from_value(json!({"serviceEpoch":service,"generation":1}))?,
                attempt_id: AttemptId::generate(),
                submissions: AtomicUsize::new(0),
                submission_plan: FakeSubmissionPlan::Accept,
                settlement_plan: FakeSettlementPlan::Pending,
            },
            turn_completed: AtomicBool::new(false),
            settlement_reads: AtomicUsize::new(0),
            stop_requests: std::sync::Mutex::default(),
            clock: Arc::clone(&clock),
            initial_evidence_fails: AtomicBool::new(false),
        });
        let instruction = store
            .lock()
            .await
            .create_instruction(
                &OperationId::generate(),
                &InstructionText::try_from("Check job".to_owned())?,
                BASE_MS,
            )
            .await?;
        store
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
                        target,
                        cwd: root.path().to_string_lossy().into_owned(),
                    },
                    execution_timeout_seconds: Some(TIMEOUT_SECONDS),
                    model: Some("fixture-model".into()),
                    effort: Some("medium".into()),
                },
                imported_continuity: ContinuityInput::None,
                now_ms: BASE_MS,
            })
            .await?;
        Ok(Self {
            _no_auto_advance: no_auto_advance,
            root,
            store,
            interactions,
            journal,
            route,
            clock,
        })
    }

    fn start(&self) -> TestResult<RunningScheduler> {
        let worker = ScheduleTimingWorker::new(
            Arc::clone(&self.store),
            Arc::clone(&self.interactions),
            Arc::clone(&self.route) as Arc<dyn ScheduledRunExecution>,
            None,
            crate::AutomationConfigurationHandle::default(),
            collaboration_contracts::MachineIdentity::new(
                "00000000-0000-4000-8000-000000000001"
                    .to_owned()
                    .try_into()?,
                Some("Schedule Fixture"),
            )?,
            Arc::clone(&self.clock) as Arc<dyn Clock>,
        )
        .wake_on_completions(self.journal.subscribe_commits());
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
        for _ in 0..WAIT_LIMIT {
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
        Err("the Run never reached a checked, executing state".into())
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

/// How many times a test waits on a condition before failing instead of hanging.
const WAIT_LIMIT: usize = 5_000;

/// Waits, without moving the clock, until the loop is asleep and stays so.
async fn settled(scheduler: &RunningScheduler) -> TestResult<()> {
    for _ in 0..WAIT_LIMIT / 100 {
        if settle(scheduler).await {
            return Ok(());
        }
    }
    Err("the scheduler never settled".into())
}

/// Lets in-flight steps finish without moving the paused clock, then reports whether the
/// loop is asleep with nothing new since.
async fn settle(scheduler: &RunningScheduler) -> bool {
    let before = scheduler.observation.snapshot();
    for _ in 0..20 {
        for _ in 0..100 {
            tokio::task::yield_now().await;
        }
        if scheduler.observation.snapshot() != before {
            return false;
        }
    }
    before.sleeps >= 1
}

/// Moves both clocks with no work in flight.
async fn advance(by: Duration) {
    tokio::time::advance(by).await;
    for _ in 0..2_000 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn an_idle_scheduler_reads_nothing_until_its_next_schedule_is_due() -> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let asleep = scheduler.observation.snapshot();

    advance(Duration::from_secs(u64::from(INTERVAL_SECONDS) - 1)).await;

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

#[tokio::test(start_paused = true)]
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

    advance(interval).await;
    let run = fixture.executing_and_asleep(&scheduler).await?;
    let armed_for_deadline = scheduler.observation.snapshot().armed_deadline;
    advance(Duration::from_secs(u64::from(TIMEOUT_SECONDS) - 1)).await;
    let stops_before_deadline = fixture.route.stop_requests.lock().unwrap().len();
    advance(Duration::from_secs(1)).await;
    settled(&scheduler).await?;
    let armed_after_deadline = scheduler.observation.snapshot().armed_deadline;
    advance(Duration::from_secs(600)).await;

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

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_running_turn_is_checked_once_and_its_completion_settles_the_run_with_no_clock_advance()
-> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    advance(Duration::from_secs(u64::from(INTERVAL_SECONDS))).await;
    let run = fixture.executing_and_asleep(&scheduler).await?;
    let checks_while_running = fixture.route.settlement_reads.load(Ordering::Acquire);

    advance(Duration::from_secs(60)).await;
    let checks_after_a_quiet_minute = fixture.route.settlement_reads.load(Ordering::Acquire);
    fixture.route.turn_completed.store(true, Ordering::Release);
    let completed_at = tokio::time::Instant::now();
    let turn_completed = thread_observation(
        json!({"kind":"turnTerminal","turnId":"fixture-turn","status":"completed"}),
    )?;
    fixture
        .journal
        .append(&turn_completed, BASE_MS / 1_000)
        .await?;
    for _ in 0..WAIT_LIMIT {
        if fixture.read_phase(&run).await? != RunPhase::Executing {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert_eq!(
        checks_after_a_quiet_minute, checks_while_running,
        "a running turn is not re-checked"
    );
    assert_eq!(fixture.read_phase(&run).await?, RunPhase::Finished);
    assert_eq!(
        tokio::time::Instant::now(),
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

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_failed_step_retries_on_the_backoff_and_recovers_without_a_new_write() -> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    fixture
        .route
        .initial_evidence_fails
        .store(true, Ordering::Release);
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let interval = Duration::from_secs(u64::from(INTERVAL_SECONDS));

    advance(interval).await;
    settled(&scheduler).await?;
    let first_retry = scheduler.observation.snapshot().armed_deadline;
    advance(Duration::from_secs(1)).await;
    settled(&scheduler).await?;
    let second_retry = scheduler.observation.snapshot().armed_deadline;
    fixture
        .route
        .initial_evidence_fails
        .store(false, Ordering::Release);
    advance(Duration::from_secs(2)).await;
    let run = fixture.executing_and_asleep(&scheduler).await?;

    assert_eq!(first_retry, Some(at(interval + Duration::from_secs(1))));
    assert_eq!(second_retry, Some(at(interval + Duration::from_secs(3))));
    assert_eq!(
        fixture.read_phase(&run).await?,
        RunPhase::Executing,
        "the retry recovered"
    );
    assert_eq!(
        scheduler.observation.snapshot().armed_deadline,
        Some(at(
            interval + Duration::from_secs(3 + u64::from(TIMEOUT_SECONDS))
        )),
        "a good pass clears the retry; the run deadline arms next"
    );
    scheduler.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_failed_read_retries_on_the_backoff_and_recovers_when_the_fault_is_fixed()
-> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    let mut fault = <sqlx::SqliteConnection as sqlx::Connection>::connect(&format!(
        "sqlite://{}",
        fixture.root.path().join("automation.sqlite").display()
    ))
    .await?;
    sqlx::query("ALTER TABLE schedule_timing_state RENAME TO schedule_timing_state_hidden")
        .execute(&mut fault)
        .await?;
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let first_retry = scheduler.observation.snapshot().armed_deadline;
    advance(Duration::from_secs(1)).await;
    settled(&scheduler).await?;
    let second_retry = scheduler.observation.snapshot().armed_deadline;
    sqlx::query("ALTER TABLE schedule_timing_state_hidden RENAME TO schedule_timing_state")
        .execute(&mut fault)
        .await?;
    advance(Duration::from_secs(2)).await;
    settled(&scheduler).await?;

    assert_eq!(first_retry, Some(at(Duration::from_secs(1))));
    assert_eq!(second_retry, Some(at(Duration::from_secs(3))));
    assert_eq!(
        scheduler.observation.snapshot().armed_deadline,
        Some(at(Duration::from_secs(u64::from(INTERVAL_SECONDS)))),
        "the recovered pass arms the schedule again"
    );
    scheduler.stop().await;
    Ok(())
}
