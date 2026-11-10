//! What a task's report tells the loop: nothing when it changed nothing and no event
//! overtook it, a wake and a sweep request when an event arrived after it started, and a
//! slot awaited after it reported is not lost.
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
        crate::worker_test_support::wait_until("the tasks reported", || {
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
async fn a_pending_task_no_event_overtook_reports_without_a_wake() -> TestResult {
    let mut fixture = tasks(16);

    fixture.tasks.spawn("run", async { true });
    fixture.reports_queued(1).await?;

    assert!(!fixture.woken(), "a step that changed nothing stays quiet");
    assert!(!fixture.tasks.take_sweep_request());
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_task_an_event_overtook_wakes_the_loop_and_asks_for_a_sweep() -> TestResult {
    let mut fixture = tasks(16);
    let gate = Arc::new(Mutex::new(()));
    let held = Arc::clone(&gate).lock_owned().await;
    fixture
        .tasks
        .hold(Some(TaskGate::new(TaskGatePoint::BeforeReport, gate)));

    fixture.tasks.spawn("run", async { true });
    fixture.tasks.event_arrived();
    let woken_while_running = fixture.woken();
    drop(held);
    fixture.reports_queued(1).await?;
    crate::worker_test_support::wait_until("the report woke the loop", || {
        fixture.wakes.has_changed().unwrap_or(false)
    })
    .await?;

    assert!(!woken_while_running, "nothing to report yet");
    assert!(
        fixture.woken(),
        "the pending report wakes the loop: what it observed may predate the event"
    );
    assert!(fixture.tasks.take_sweep_request(), "and asks for a sweep");
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_task_started_after_the_event_reports_without_a_wake() -> TestResult {
    let mut fixture = tasks(16);

    fixture.tasks.event_arrived();
    fixture.tasks.spawn("run", async { true });
    fixture.reports_queued(1).await?;

    assert!(!fixture.woken(), "it observed after the event");
    assert!(!fixture.tasks.take_sweep_request());
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
    assert!(!fixture.tasks.is_busy(&"run", NOW), "the slot is free");
    Ok(())
}
