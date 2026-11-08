//! The loop on Tokio's paused clock with the wall clock that moves with it.
use super::{BatchOutcome, WakeDrivenWorkerLoop, WorkerPass};
use crate::{WorkerObservation, paused_wall_clock::PausedWallClock};
use runtime_primitives::{Clock, SyncTime};
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
    /// Held only when the pass itself commits; otherwise the test owns the only sender.
    commits: Option<watch::Sender<u64>>,
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
            if let Some(commits) = &self.commits {
                commits.send_modify(|revision| *revision += 1);
            }
        }
        Ok(work.due.map(|(due, _)| due))
    }
}

struct RunningLoop {
    work: Arc<Mutex<FakeWork>>,
    clock: Arc<PausedWallClock>,
    commits: watch::Sender<u64>,
    observation: WorkerObservation,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl RunningLoop {
    fn start(work: FakeWork) -> Self {
        let pass_commits = work.committed_during_next_due.is_some();
        let work = Arc::new(Mutex::new(work));
        let (commits, receiver) = watch::channel(0);
        let clock = Arc::new(PausedWallClock::starting_at(at(Duration::ZERO)));
        let worker = WakeDrivenWorkerLoop::new("fake", clock.clone()).wake_on(receiver);
        let observation = worker.observation();
        let shutdown = CancellationToken::new();
        let mut pass = FakePass {
            work: Arc::clone(&work),
            commits: pass_commits.then(|| commits.clone()),
        };
        let task = tokio::spawn({
            let shutdown = shutdown.clone();
            async move { worker.run(&mut pass, shutdown).await }
        });
        Self {
            work,
            clock,
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

    fn clock_now(&self) -> SyncTime {
        self.clock.now()
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

/// Collects the WARN events the loop emits, with their worker and error fields.
#[derive(Clone, Default)]
struct CapturedWarnings(Arc<Mutex<Vec<(String, String)>>>);

impl<TSubscriber: tracing::Subscriber> tracing_subscriber::Layer<TSubscriber> for CapturedWarnings {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _: tracing_subscriber::layer::Context<'_, TSubscriber>,
    ) {
        if *event.metadata().level() != tracing::Level::WARN {
            return;
        }
        let mut fields = WarningFields::default();
        event.record(&mut fields);
        self.0.lock().unwrap().push((fields.worker, fields.error));
    }
}

#[derive(Default)]
struct WarningFields {
    worker: String,
    error: String,
}

impl tracing::field::Visit for WarningFields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "worker" {
            self.worker = value.to_owned();
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "worker" => self.worker = format!("{value:?}").trim_matches('"').to_owned(),
            "error" => self.error = format!("{value:?}"),
            _ => {}
        }
    }
}

/// The armed retry delays, in seconds, for `sleeps` sleeps starting at sleep `first`.
async fn retry_delays(
    worker: &RunningLoop,
    first: u64,
    sleeps: u64,
    started: SyncTime,
) -> Vec<i64> {
    let mut delays = Vec::new();
    let mut pass_started = started;
    for sleep in first..first + sleeps {
        worker.observation.asleep_at_least(sleep).await;
        let armed = worker.observation.snapshot().armed_deadline.unwrap();
        delays.push((armed.unix_millis() - pass_started.unix_millis()) / 1_000);
        pass_started = armed;
    }
    delays
}

#[tokio::test(start_paused = true)]
async fn a_failed_pass_retries_at_one_two_four_seconds_up_to_thirty_and_logs_once_per_run() {
    let warnings = CapturedWarnings::default();
    let _subscriber =
        tracing::subscriber::set_default(tracing_subscriber::layer::SubscriberExt::with(
            tracing_subscriber::registry(),
            warnings.clone(),
        ));
    let worker = RunningLoop::start(FakeWork {
        failing_passes: 7,
        ..FakeWork::default()
    });

    let first_run = retry_delays(&worker, 1, 7, at(Duration::ZERO)).await;
    worker.observation.asleep_at_least(8).await;
    let recovered = worker.observation.snapshot();
    let warnings_after_first_run = warnings.0.lock().unwrap().clone();
    worker.work.lock().unwrap().failing_passes = 2;
    worker.commit_batch("after recovery");
    let second_started = worker.clock_now();
    let second_run = retry_delays(&worker, 9, 2, second_started).await;
    worker.observation.asleep_at_least(11).await;

    assert_eq!(first_run, [1, 2, 4, 8, 16, 30, 30]);
    assert_eq!(
        recovered.armed_deadline, None,
        "a good pass clears the retry"
    );
    assert_eq!(
        warnings_after_first_run,
        [("fake".to_owned(), "injected read failure".to_owned())],
        "one warning, with its worker and error, for a run of seven failures"
    );
    assert_eq!(second_run, [1, 2], "the backoff restarts after recovery");
    assert_eq!(
        warnings.0.lock().unwrap().len(),
        2,
        "a second run of failures logs once more"
    );
    assert_eq!(worker.observation.snapshot().armed_deadline, None);
    worker.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_closed_source_is_removed_and_does_not_keep_waking_the_loop() {
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
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }
    // Checked before any sleep: a loop that kept a closed source would never let the
    // paused runtime go idle, so a sleep here would hang instead of failing.
    assert_eq!(
        observation.snapshot().closed_sources,
        1,
        "the closed source is removed once"
    );
    tokio::time::sleep(Duration::from_secs(3600)).await;

    assert_eq!(observation.snapshot().passes, 1, "closing wakes no pass");
    shutdown.cancel();
    task.await.unwrap();
}

/// Always has more work. It cancels shutdown during the batch it was told to, and stops
/// at a cap so a loop that ignores shutdown fails the test instead of hanging it.
struct EndlessWork {
    batches: Arc<Mutex<u32>>,
    cancel_during_batch: Option<u32>,
    shutdown: CancellationToken,
}

const ENDLESS_WORK_CAP: u32 = 1_000;

impl WorkerPass for EndlessWork {
    type Error = &'static str;

    async fn drain_batch(
        &mut self,
        _now: SyncTime,
        _observation: &WorkerObservation,
    ) -> Result<BatchOutcome, &'static str> {
        let batch = {
            let mut batches = self.batches.lock().map_err(|_| "poisoned")?;
            *batches += 1;
            *batches
        };
        if self.cancel_during_batch == Some(batch) {
            self.shutdown.cancel();
        }
        tokio::task::yield_now().await;
        Ok(if batch < ENDLESS_WORK_CAP {
            BatchOutcome::MoreWork
        } else {
            BatchOutcome::Drained
        })
    }

    async fn next_due(
        &mut self,
        _now: SyncTime,
        _observation: &WorkerObservation,
    ) -> Result<Option<SyncTime>, &'static str> {
        Ok(None)
    }
}

async fn batches_run_with(cancel_during_batch: Option<u32>, shutdown: CancellationToken) -> u32 {
    let batches = Arc::new(Mutex::new(0));
    let mut pass = EndlessWork {
        batches: Arc::clone(&batches),
        cancel_during_batch,
        shutdown: shutdown.clone(),
    };
    let worker = WakeDrivenWorkerLoop::new(
        "endless",
        Arc::new(PausedWallClock::starting_at(at(Duration::ZERO))),
    );
    let started = tokio::time::Instant::now();

    worker.run(&mut pass, shutdown).await;

    assert_eq!(
        tokio::time::Instant::now(),
        started,
        "shutdown needs no clock advance"
    );
    let ran = *batches.lock().unwrap();
    ran
}

#[tokio::test(start_paused = true)]
async fn shutdown_during_a_batch_lets_it_finish_and_starts_no_other() {
    let ran = batches_run_with(Some(1), CancellationToken::new()).await;

    assert_eq!(ran, 1, "no batch starts after shutdown");
}

#[tokio::test(start_paused = true)]
async fn a_loop_started_after_shutdown_runs_no_pass() {
    let shutdown = CancellationToken::new();
    shutdown.cancel();

    let ran = batches_run_with(None, shutdown).await;

    assert_eq!(ran, 0, "a cancelled worker starts no pass");
}
