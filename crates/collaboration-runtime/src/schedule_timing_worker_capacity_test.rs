//! More observable Runs than step slots and than one inventory batch: the scheduler steps
//! each Run once, sixteen at a time, with no clock advance, and reads past a batch whose
//! Runs are all waiting for their retry.
use super::{
    INTERVAL_SECONDS, SchedulerFixture, TestResult, advance, observe, settled, settled_holding,
    wait_until,
};
use agent_automation::RunId;
use runtime_primitives::Clock;
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Mutex;

/// More Runs than one inventory batch (100) and than the step slots (16).
const RUN_COUNT: usize = 120;
const INVENTORY_BATCH: usize = 100;
const STEP_SLOTS: usize = 16;

impl SchedulerFixture {
    /// Copies an executing Run `copies` times under new UUIDv7 ids, which sort after
    /// every existing Run.
    async fn copy_executing_run(&self, run: &RunId, copies: usize) -> TestResult<Vec<RunId>> {
        let mut connection = <sqlx::SqliteConnection as sqlx::Connection>::connect(&format!(
            "sqlite://{}",
            self.root.path().join("automation.sqlite").display()
        ))
        .await?;
        let mut runs = Vec::new();
        while runs.len() < copies {
            let copy = RunId::generate();
            sqlx::query(
                "INSERT INTO workflow_runs SELECT ?, schedule_id, due_at_ms, run_status, \
                 captured_inputs_json, thread_binding_id, native_turn_id, \
                 execution_started_at_ms, execution_deadline_at_ms, effective_timeout_seconds, \
                 execution_evidence_json, worker_outcome_json, summary_attempt_json, \
                 summary_text, summary_source_json, completed_at_ms \
                 FROM workflow_runs WHERE run_id = ?",
            )
            .bind(copy.as_str())
            .bind(run.as_str())
            .execute(&mut connection)
            .await?;
            runs.push(copy);
        }
        Ok(runs)
    }

    /// One executing Run, reached through the real scheduler, which is stopped again.
    async fn one_executing_run(&self) -> TestResult<RunId> {
        let first = self.start()?;
        settled(&first).await?;
        advance(Duration::from_secs(u64::from(INTERVAL_SECONDS))).await;
        let run = self.executing_and_asleep(&first).await?;
        first.stop().await;
        self.route.observed_runs.lock().unwrap().clear();
        Ok(run)
    }

    fn observed_runs(&self) -> Vec<RunId> {
        self.route.observed_runs.lock().unwrap().clone()
    }

    /// Waits, without moving the clock, until `count` settlement reads happened. It fails
    /// once no read happened for a whole wait limit.
    async fn observed(&self, count: usize) -> TestResult<()> {
        wait_until("every expected settlement read happened", || {
            self.observed_runs().len() >= count
        })
        .await
    }
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn more_runs_than_slots_and_than_a_batch_are_each_stepped_once_with_no_clock_advance()
-> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    let run = fixture.one_executing_run().await?;
    let mut runs = fixture.copy_executing_run(&run, RUN_COUNT - 1).await?;
    runs.push(run);
    let started_at = fixture.clock.now();
    let gate = Arc::new(Mutex::new(()));
    let held = Arc::clone(&gate).lock_owned().await;

    // Every step finds its turn still running, writes nothing and is held before it
    // reports, so all slots stay taken.
    let scheduler = fixture.start_with(|worker| worker.hold_step_reports(gate))?;
    fixture.observed(STEP_SLOTS).await?;
    settled_holding(&scheduler.observation).await?;
    let while_held = fixture.observed_runs().len();
    drop(held);
    fixture.observed(RUN_COUNT).await?;
    settled(&scheduler).await?;
    let mut observed = fixture.observed_runs();
    observed.sort();
    runs.sort();

    assert_eq!(while_held, STEP_SLOTS, "no more than sixteen steps at once");
    assert_eq!(observed, runs, "every Run stepped exactly once");
    assert_eq!(fixture.clock.now(), started_at, "with no clock advance");
    scheduler.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn runs_after_a_full_batch_waiting_for_retries_are_stepped_with_no_clock_advance()
-> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    let run = fixture.one_executing_run().await?;
    fixture
        .copy_executing_run(&run, INVENTORY_BATCH - 1)
        .await?;
    fixture
        .route
        .unreachable_settlements
        .store(INVENTORY_BATCH, Ordering::Release);
    let scheduler = fixture.start()?;
    fixture.observed(INVENTORY_BATCH).await?;
    settled(&scheduler).await?;
    let started_at = fixture.clock.now();

    // The first batch of 100 Runs all wait for their retry; the Runs after it are new.
    let mut later = fixture
        .copy_executing_run(&run, RUN_COUNT - INVENTORY_BATCH)
        .await?;
    observe(
        &fixture,
        json!({"kind":"threadStatus","status":{"type":"idle"},"ordering":"established"}),
    )
    .await?;
    fixture.observed(RUN_COUNT).await?;
    settled(&scheduler).await?;
    let mut stepped_later = fixture.observed_runs().split_off(INVENTORY_BATCH);
    stepped_later.sort();
    later.sort();

    assert_eq!(
        stepped_later, later,
        "a batch of Runs all waiting for their retry does not hide the Runs after it"
    );
    assert_eq!(fixture.clock.now(), started_at, "with no clock advance");
    scheduler.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn reports_queued_before_the_scheduler_waits_for_a_slot_are_not_lost() -> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    let run = fixture.one_executing_run().await?;
    let mut runs = fixture.copy_executing_run(&run, RUN_COUNT - 1).await?;
    runs.push(run);
    let started_at = fixture.clock.now();

    // When the slots first fill, every one of those steps reports (unchanged, so
    // quietly) before the scheduler marks that it waits for a slot.
    let scheduler = fixture.start_with(|worker| worker.hold_full_table_until_reports_queued())?;
    fixture.observed(RUN_COUNT).await?;
    settled(&scheduler).await?;
    let mut observed = fixture.observed_runs();
    observed.sort();
    runs.sort();

    assert_eq!(observed, runs, "every Run stepped exactly once");
    assert_eq!(fixture.clock.now(), started_at, "with no clock advance");
    scheduler.stop().await;
    Ok(())
}

/// Runs whose turn a test completed one by one.
#[derive(Default)]
pub(super) struct CompletedRuns(std::sync::Mutex<std::collections::HashSet<RunId>>);

impl CompletedRuns {
    pub(super) fn contains(&self, run: &RunId) -> bool {
        self.0.lock().unwrap().contains(run)
    }

    fn complete(&self, run: RunId) {
        self.0.lock().unwrap().insert(run);
    }
}

/// 120 executing Runs, ordered by id, and a scheduler whose step reports are held and that
/// wakes on a completion source the test announces.
struct CursorFixture {
    fixture: SchedulerFixture,
    runs: Vec<RunId>,
    gate: Arc<Mutex<()>>,
    arrivals: Arc<AtomicUsize>,
    completions: tokio::sync::watch::Sender<u64>,
    scheduler: super::RunningScheduler,
}

impl CursorFixture {
    async fn start(gate: Arc<Mutex<()>>) -> TestResult<Self> {
        let fixture = SchedulerFixture::with_one_schedule().await?;
        let run = fixture.one_executing_run().await?;
        let mut runs = fixture.copy_executing_run(&run, RUN_COUNT - 1).await?;
        runs.push(run);
        runs.sort();
        let arrivals = Arc::new(AtomicUsize::new(0));
        let (completions, completion_source) = tokio::sync::watch::channel(0);
        let scheduler = fixture.start_with(|worker| {
            worker
                .hold_step_reports_counting(Arc::clone(&gate), Arc::clone(&arrivals))
                .wake_on_completions(completion_source)
        })?;
        Ok(Self {
            fixture,
            runs,
            gate,
            arrivals,
            completions,
            scheduler,
        })
    }

    async fn held(&self, count: usize) -> TestResult<()> {
        wait_until("the expected steps were held", || {
            self.arrivals.load(Ordering::SeqCst) >= count
        })
        .await?;
        settled_holding(&self.scheduler.observation).await
    }

    /// Completes `run`'s turn and announces it, as a completion source would, then waits
    /// until a pass consumed the announcement and the scheduler slept again.
    async fn complete(&self, run: &RunId) -> TestResult<()> {
        let sleeps = self.scheduler.observation.snapshot().sleeps;
        self.fixture.route.completed_runs.complete(run.clone());
        self.completions
            .send_modify(|revision| *revision = revision.wrapping_add(1));
        wait_until("the completion woke a pass", || {
            self.scheduler.observation.snapshot().sleeps > sleeps
        })
        .await?;
        settled_holding(&self.scheduler.observation).await
    }

    /// Waits for `run` to leave Executing, then for the scheduler to settle.
    async fn settles(&self, run: &RunId) -> TestResult<super::RunPhase> {
        let limit = super::CollectionLimit::start("the completed Run settled");
        while self.fixture.read_phase(run).await? == super::RunPhase::Executing {
            limit.next().await?;
        }
        settled(&self.scheduler).await?;
        self.fixture.read_phase(run).await
    }
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_completion_for_a_run_in_flight_behind_a_full_table_is_checked_again() -> TestResult<()> {
    let gate = Arc::new(Mutex::new(()));
    let held = Arc::clone(&gate).lock_owned().await;
    let cursor = CursorFixture::start(Arc::clone(&gate)).await?;
    let target = cursor.runs[4].clone();
    let started_at = cursor.fixture.clock.now();

    // The first sixteen Runs, the target among them, are held after reading their turn
    // running; the table is full and the scan has moved past them.
    cursor.held(STEP_SLOTS).await?;
    cursor.complete(&target).await?;
    let phase_while_held = cursor.fixture.read_phase(&target).await?;
    drop(held);
    let phase = cursor.settles(&target).await?;

    assert_eq!(phase_while_held, super::RunPhase::Executing);
    assert_eq!(
        phase,
        super::RunPhase::Finished,
        "the Run in flight when its completion arrived is checked again"
    );
    assert_eq!(
        cursor.fixture.clock.now(),
        started_at,
        "with no clock advance"
    );
    cursor.scheduler.stop().await;
    drop(cursor.gate);
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_completion_for_an_idle_run_the_scan_has_passed_is_checked_again() -> TestResult<()> {
    let gate = Arc::new(Mutex::new(()));
    let held = Arc::clone(&gate).lock_owned().await;
    let cursor = CursorFixture::start(Arc::clone(&gate)).await?;
    let target = cursor.runs[4].clone();
    let started_at = cursor.fixture.clock.now();

    // The first sixteen Runs report their turns running and go idle; the next sixteen
    // are held, so the table is full and the scan has passed the target.
    cursor.held(STEP_SLOTS).await?;
    drop(held);
    // The gate is fair: the first sixteen pass before this lock, the next wave waits.
    let held = Arc::clone(&gate).lock_owned().await;
    cursor.held(2 * STEP_SLOTS).await?;
    cursor.complete(&target).await?;
    let phase_while_held = cursor.fixture.read_phase(&target).await?;
    drop(held);
    let phase = cursor.settles(&target).await?;

    assert_eq!(phase_while_held, super::RunPhase::Executing);
    assert_eq!(
        phase,
        super::RunPhase::Finished,
        "the sweep after the current one checks the idle Run"
    );
    assert_eq!(
        cursor.fixture.clock.now(),
        started_at,
        "with no clock advance"
    );
    cursor.scheduler.stop().await;
    drop(cursor.gate);
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_completion_that_resumes_a_sweep_past_its_run_is_followed_by_a_full_sweep()
-> TestResult<()> {
    const RUNS: usize = STEP_SLOTS + 4;
    let fixture = SchedulerFixture::with_one_schedule().await?;
    let run = fixture.one_executing_run().await?;
    let mut runs = fixture.copy_executing_run(&run, RUNS - 1).await?;
    runs.push(run);
    runs.sort();
    let target = runs[4].clone();
    let (completions, completion_source) = tokio::sync::watch::channel(0_u64);
    let mut first_sleep = None;
    let scheduler = fixture.start_with(|worker| {
        first_sleep = Some(worker.observation().hold_before_next_sleep());
        worker.wake_on_completions(completion_source)
    })?;
    let mut first_sleep = first_sleep.ok_or("the sleep gate was set")?;
    let started_at = fixture.clock.now();

    // The first pass steps the first sixteen Runs and stops at the full table. Held
    // before it sleeps, every one of those steps reports its turn running; then the
    // target's turn completes. The pass this wakes resumes the scan after the target,
    // takes the reports, reaches the last Run with nothing left in flight to report.
    first_sleep.reached().await;
    wait_until("the first sixteen steps reported", || {
        scheduler.observation.snapshot().tasks_running == 0
            && fixture.observed_runs().len() >= STEP_SLOTS
    })
    .await?;
    fixture.route.completed_runs.complete(target.clone());
    completions.send_modify(|revision| *revision = revision.wrapping_add(1));
    first_sleep.release();
    let limit = super::CollectionLimit::start("the completed Run settled");
    while fixture.read_phase(&target).await? == super::RunPhase::Executing {
        limit.next().await?;
    }
    settled(&scheduler).await?;

    assert_eq!(
        fixture.read_phase(&target).await?,
        super::RunPhase::Finished,
        "the sweep the completion was owed reaches the Run the resumed scan had passed"
    );
    assert_eq!(fixture.clock.now(), started_at, "with no clock advance");
    scheduler.stop().await;
    Ok(())
}
