//! The orders between a task's report and the loop's marks: a recheck owed while the task
//! runs, a recheck asked for after it reported, and a slot awaited after it reported.
use super::{KeyedWorkerTasks, TaskGate, TaskGatePoint};
use runtime_primitives::SyncTime;
use std::sync::Arc;
use tokio::sync::{Mutex, watch};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

const NOW: SyncTime = SyncTime::from_unix_millis(1_800_000_000_000);

struct Tasks {
    tasks: KeyedWorkerTasks<&'static str>,
    wakes: watch::Receiver<u64>,
    _commits: watch::Sender<u64>,
}

fn tasks(capacity: usize) -> Tasks {
    let (commits, receiver) = watch::channel(0);
    let (tasks, mut wakes) = KeyedWorkerTasks::new(capacity, receiver, "fixture task failed");
    wakes.borrow_and_update();
    Tasks {
        tasks,
        wakes,
        _commits: commits,
    }
}

impl Tasks {
    /// Waits until `count` reports are queued.
    async fn reports_queued(&self, count: usize) -> TestResult {
        crate::paused_worker_test_support::wait_until("the tasks reported", || {
            self.tasks.finished.len() >= count
        })
        .await
    }

    fn woken(&mut self) -> bool {
        let woken = self.wakes.has_changed().unwrap_or(false);
        self.wakes.borrow_and_update();
        woken
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_pending_task_nobody_asked_about_reports_without_a_wake() -> TestResult {
    let mut fixture = tasks(16);

    fixture.tasks.spawn("run", async { true });
    fixture.reports_queued(1).await?;

    assert!(!fixture.woken(), "a step that changed nothing stays quiet");
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_task_skipped_while_it_runs_wakes_the_loop_when_it_reports() -> TestResult {
    let mut fixture = tasks(16);
    let gate = Arc::new(Mutex::new(()));
    let held = Arc::clone(&gate).lock_owned().await;
    fixture
        .tasks
        .hold(Some(TaskGate::new(TaskGatePoint::BeforeReport, gate)));

    fixture.tasks.spawn("run", async { true });
    let skipped = fixture.tasks.skip_busy(&"run", NOW);
    let woken_while_running = fixture.woken();
    drop(held);
    fixture.reports_queued(1).await?;

    assert!(skipped);
    assert!(!woken_while_running, "nothing to recheck yet");
    assert!(
        fixture.woken(),
        "the pending report wakes the loop for the recheck it was owed"
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_task_skipped_after_it_reported_wakes_the_loop_at_once() -> TestResult {
    let mut fixture = tasks(16);

    fixture.tasks.spawn("run", async { true });
    fixture.reports_queued(1).await?;
    let quiet_report = fixture.woken();
    let skipped = fixture.tasks.skip_busy(&"run", NOW);

    assert!(!quiet_report);
    assert!(skipped, "its report is queued but not yet taken");
    assert!(
        fixture.woken(),
        "the loop runs again to take the report and recheck the key"
    );
    assert!(
        fixture.tasks.take_sweep_request(),
        "a pass past the key sweeps again"
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_full_table_whose_reports_are_already_queued_is_not_full() -> TestResult {
    let mut fixture = tasks(1);

    fixture.tasks.spawn("run", async { true });
    fixture.reports_queued(1).await?;
    let quiet_report = fixture.woken();
    let full = fixture.tasks.at_capacity(NOW).await;

    assert!(!quiet_report, "the report came before the loop waited");
    assert!(!full, "the loop takes the queued report instead of waiting");
    assert!(!fixture.tasks.skip_busy(&"run", NOW), "the slot is free");
    Ok(())
}
