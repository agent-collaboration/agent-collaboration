//! The loop on Tokio's paused clock with the wall clock that moves with it.
use super::{BatchOutcome, WakeDrivenWorkerLoop, WorkerPass};
use crate::{WorkerObservation, paused_wall_clock::PausedWallClock};
use runtime_primitives::SyncTime;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

const BASE_MS: i64 = 1_800_000_000_000;

fn at(offset: Duration) -> SyncTime {
    SyncTime::from_unix_millis(BASE_MS).after(offset)
}

#[derive(Default)]
struct FakeWork {
    batches: VecDeque<&'static str>,
    due: Option<(SyncTime, &'static str)>,
    failing_passes: u32,
    /// Another writer commits this batch while the pass reads its next due time, after
    /// its batch read came back empty.
    committed_during_next_due: Option<&'static str>,
    handled: Vec<(&'static str, SyncTime)>,
}

struct FakePass {
    work: Arc<Mutex<FakeWork>>,
    commits: watch::Sender<u64>,
}

impl WorkerPass for FakePass {
    type Error = &'static str;

    async fn drain_batch(
        &mut self,
        now: SyncTime,
        observation: &WorkerObservation,
    ) -> Result<BatchOutcome, &'static str> {
        observation.record_read("fake batch");
        let mut work = self.work.lock().map_err(|_| "poisoned")?;
        if work.failing_passes > 0 {
            work.failing_passes -= 1;
            return Err("injected read failure");
        }
        if let Some((due, name)) = work.due
            && due <= now
        {
            work.due = None;
            work.handled.push((name, now));
        }
        match work.batches.pop_front() {
            Some(name) => {
                work.handled.push((name, now));
                Ok(BatchOutcome::MoreWork)
            }
            None => Ok(BatchOutcome::Drained),
        }
    }

    async fn next_due(
        &mut self,
        _now: SyncTime,
        observation: &WorkerObservation,
    ) -> Result<Option<SyncTime>, &'static str> {
        observation.record_read("fake next due");
        let mut work = self.work.lock().map_err(|_| "poisoned")?;
        if let Some(name) = work.committed_during_next_due.take() {
            work.batches.push_back(name);
            self.commits.send_modify(|revision| *revision += 1);
        }
        Ok(work.due.map(|(due, _)| due))
    }
}

struct RunningLoop {
    work: Arc<Mutex<FakeWork>>,
    commits: watch::Sender<u64>,
    observation: WorkerObservation,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl RunningLoop {
    fn start(work: FakeWork) -> Self {
        let work = Arc::new(Mutex::new(work));
        let (commits, receiver) = watch::channel(0);
        let worker = WakeDrivenWorkerLoop::new(
            "fake",
            Arc::new(PausedWallClock::starting_at(at(Duration::ZERO))),
        )
        .wake_on(receiver);
        let observation = worker.observation();
        let shutdown = CancellationToken::new();
        let mut pass = FakePass {
            work: Arc::clone(&work),
            commits: commits.clone(),
        };
        let task = tokio::spawn({
            let shutdown = shutdown.clone();
            async move { worker.run(&mut pass, shutdown).await }
        });
        Self {
            work,
            commits,
            observation,
            shutdown,
            task,
        }
    }

    /// A committed write that creates one batch of work.
    fn commit_batch(&self, name: &'static str) {
        self.work.lock().unwrap().batches.push_back(name);
        self.commits.send_modify(|revision| *revision += 1);
    }

    fn handled(&self) -> Vec<(&'static str, SyncTime)> {
        self.work.lock().unwrap().handled.clone()
    }

    async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn startup_drains_every_committed_batch_then_sleeps_with_no_timer() {
    let worker = RunningLoop::start(FakeWork {
        batches: VecDeque::from(["first", "second", "third"]),
        ..FakeWork::default()
    });

    worker.observation.asleep_at_least(1).await;

    let start = at(Duration::ZERO);
    assert_eq!(
        worker.handled(),
        [("first", start), ("second", start), ("third", start)]
    );
    let snapshot = worker.observation.snapshot();
    assert_eq!(snapshot.passes, 1);
    assert_eq!(snapshot.armed_deadline, None);
    worker.stop().await;
}

#[tokio::test(start_paused = true)]
async fn an_idle_hour_makes_no_reads_and_holds_no_timer() {
    let worker = RunningLoop::start(FakeWork::default());
    worker.observation.asleep_at_least(1).await;
    let asleep = worker.observation.snapshot();

    tokio::time::sleep(Duration::from_secs(3600)).await;

    let after_an_hour = worker.observation.snapshot();
    assert_eq!(asleep.reads, ["fake batch", "fake next due"]);
    assert_eq!(after_an_hour, asleep, "an idle loop reads nothing");
    assert_eq!(after_an_hour.armed_deadline, None);
    worker.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_commit_wakes_the_sleeping_loop_without_a_clock_advance() {
    let worker = RunningLoop::start(FakeWork::default());
    worker.observation.asleep_at_least(1).await;

    worker.commit_batch("committed");
    worker.observation.asleep_at_least(2).await;

    assert_eq!(worker.handled(), [("committed", at(Duration::ZERO))]);
    worker.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_commit_between_the_empty_read_and_sleep_is_still_handled() {
    let worker = RunningLoop::start(FakeWork::default());
    let mut gate = worker.observation.hold_before_next_sleep();
    gate.reached().await;

    worker.commit_batch("raced");
    gate.release();
    worker.observation.asleep_at_least(2).await;

    assert_eq!(worker.handled(), [("raced", at(Duration::ZERO))]);
    assert_eq!(worker.observation.snapshot().passes, 2);
    worker.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_commit_during_the_pass_after_its_empty_read_is_handled_before_sleeping() {
    let worker = RunningLoop::start(FakeWork {
        committed_during_next_due: Some("during the pass"),
        ..FakeWork::default()
    });

    worker.observation.asleep_at_least(1).await;

    assert_eq!(worker.handled(), [("during the pass", at(Duration::ZERO))]);
    assert_eq!(worker.observation.snapshot().passes, 2);
    worker.stop().await;
}

#[tokio::test(start_paused = true)]
async fn due_work_runs_at_its_due_time_on_one_timer_and_does_not_fire_again() {
    let due = at(Duration::from_secs(10));
    let worker = RunningLoop::start(FakeWork {
        due: Some((due, "due")),
        ..FakeWork::default()
    });
    worker.observation.asleep_at_least(1).await;
    let armed = worker.observation.snapshot().armed_deadline;

    worker.observation.asleep_at_least(2).await;
    let after_due = worker.observation.snapshot();
    tokio::time::sleep(Duration::from_secs(3600)).await;

    assert_eq!(armed, Some(due));
    assert_eq!(
        worker.handled(),
        [("due", due)],
        "runs at the due time, not before"
    );
    assert_eq!(
        after_due.armed_deadline, None,
        "an acted-on deadline is gone"
    );
    assert_eq!(worker.observation.snapshot(), after_due);
    worker.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_failed_pass_retries_at_one_two_four_seconds_up_to_thirty_and_logs_once() {
    let worker = RunningLoop::start(FakeWork {
        failing_passes: 7,
        ..FakeWork::default()
    });
    let mut retry_delays = Vec::new();
    let mut pass_started = at(Duration::ZERO);

    for sleeps in 1..=7 {
        worker.observation.asleep_at_least(sleeps).await;
        let armed = worker.observation.snapshot().armed_deadline.unwrap();
        retry_delays.push((armed.unix_millis() - pass_started.unix_millis()) / 1_000);
        pass_started = armed;
    }
    worker.observation.asleep_at_least(8).await;
    let recovered = worker.observation.snapshot();

    assert_eq!(retry_delays, [1, 2, 4, 8, 16, 30, 30]);
    assert_eq!(
        recovered.failure_reports, 1,
        "one log line per run of failures"
    );
    assert_eq!(
        recovered.armed_deadline, None,
        "a good pass clears the retry"
    );
    worker.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_closed_source_does_not_keep_waking_the_loop() {
    let worker = RunningLoop::start(FakeWork::default());
    worker.observation.asleep_at_least(1).await;
    let RunningLoop {
        commits,
        observation,
        shutdown,
        task,
        ..
    } = worker;

    drop(commits);
    tokio::time::sleep(Duration::from_secs(3600)).await;

    assert_eq!(observation.snapshot().passes, 1);
    shutdown.cancel();
    task.await.unwrap();
}
