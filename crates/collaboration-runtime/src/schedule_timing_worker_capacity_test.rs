//! More observable Runs than step slots and than one inventory batch: the scheduler steps
//! each Run once, sixteen at a time, with no clock advance, and reads past a batch whose
//! Runs are all waiting for their retry.
use super::{
    INTERVAL_SECONDS, SchedulerFixture, TestResult, WAIT_LIMIT, advance, observe, settled,
};
use agent_automation::RunId;
use runtime_primitives::Clock;
use serde_json::json;
use std::{
    sync::{Arc, atomic::Ordering},
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
        let mut seen = self.observed_runs().len();
        let mut idle = 0;
        while idle < WAIT_LIMIT {
            let now_seen = self.observed_runs().len();
            if now_seen >= count {
                return Ok(());
            }
            if now_seen == seen {
                idle += 1;
            } else {
                seen = now_seen;
                idle = 0;
            }
            tokio::task::yield_now().await;
        }
        Err(format!("settlement reads stalled at {seen} of {count}").into())
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
    settled(&scheduler).await?;
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
