//! A failed owner pass, and a failed held-push clean-up, retry on a 1 s to 30 s backoff
//! and log once per run of failures.
use super::super::subscription_service::OwnerObservation;
use super::owner_fixture::*;
use super::startup_fixture::{exhaust_project_readers, release_project_readers};
use router_clock::RuntimeClock;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use tracing_subscriber::{Layer, layer::SubscriberExt};

const BACKOFF_SECONDS: [u64; 7] = [1, 2, 4, 8, 16, 30, 30];

/// How far short of each deadline the clock stops to show no retry has happened yet.
const JUST_BEFORE: Duration = Duration::from_millis(500);

/// Counts each value a named structured field takes across log events.
struct RetryFieldLog {
    field: &'static str,
    counts: Arc<StdMutex<HashMap<String, usize>>>,
}
impl<TSubscriber: tracing::Subscriber> Layer<TSubscriber> for RetryFieldLog {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _: tracing_subscriber::layer::Context<'_, TSubscriber>,
    ) {
        struct Visitor<'log>(&'log RetryFieldLog);
        impl tracing::field::Visit for Visitor<'_> {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == self.0.field {
                    *self
                        .0
                        .counts
                        .lock()
                        .unwrap()
                        .entry(value.to_owned())
                        .or_default() += 1;
                }
            }
            fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
        }
        event.record(&mut Visitor(self));
    }
}

fn retry_log(field: &'static str) -> (Arc<StdMutex<HashMap<String, usize>>>, impl Drop) {
    let counts = Arc::new(StdMutex::new(HashMap::new()));
    let subscriber = tracing_subscriber::registry().with(RetryFieldLog {
        field,
        counts: counts.clone(),
    });
    (counts, tracing::subscriber::set_default(subscriber))
}

fn logged(counts: &StdMutex<HashMap<String, usize>>, value: &str) -> usize {
    counts.lock().unwrap().get(value).copied().unwrap_or(0)
}

type OwnerEvents = tokio::sync::broadcast::Receiver<OwnerObservation>;

/// The deadline an owner arms after a failed attempt; each failed pass ends in one sleep.
/// The bound only stops a hang; it outlasts the paused test's storage acquire timeout.
async fn next_failed_attempt(events: &mut OwnerEvents) -> router_clock::ClockInstant {
    (async {
        loop {
            if let OwnerObservation::Sleeping(Some(deadline)) = events.recv().await.unwrap() {
                return deadline;
            }
        }
    })
    .await
}

/// Stops the clock just short of `deadline` and requires the sleeper to look and keep
/// waiting with no attempt, then moves the clock exactly to the deadline.
async fn step_to_deadline<TAttempt: std::fmt::Debug>(
    clock: &ManualClock,
    deadline: router_clock::ClockInstant,
    early_attempt: impl std::future::Future<Output = TAttempt>,
) {
    let remaining = deadline.saturating_duration_since(clock.monotonic_now());
    clock.advance(remaining - JUST_BEFORE).unwrap();
    tokio::select! {
        () = clock.not_due_check_now(deadline) => {}
        attempt = early_attempt => {
            panic!("retried {JUST_BEFORE:?} before its deadline: {attempt:?}")
        }
    }
    clock.advance(JUST_BEFORE).unwrap();
}

/// Walks the owner through one full backoff schedule from its first failed attempt and
/// returns the deadline the last attempt armed. Each step's delay is exact, no attempt
/// happens just before it, and exactly one happens at it.
async fn walk_owner_backoff(
    runtime: &OwnerRuntime,
    clock: &ManualClock,
) -> router_clock::ClockInstant {
    let mut events = runtime.observations.lock().await;
    let mut deadline = next_failed_attempt(&mut events).await;
    let mut armed = Vec::new();
    for _ in BACKOFF_SECONDS {
        armed.push(deadline.saturating_duration_since(clock.monotonic_now()));
        let expected: Vec<Duration> = BACKOFF_SECONDS[..armed.len()]
            .iter()
            .map(|seconds| Duration::from_secs(*seconds))
            .collect();
        assert_eq!(armed, expected);
        step_to_deadline(clock, deadline, next_failed_attempt(&mut events)).await;
        deadline = next_failed_attempt(&mut events).await;
    }
    deadline
}

#[tokio::test]
async fn a_failed_owner_pass_backs_off_from_one_to_thirty_seconds_and_logs_once_per_run() {
    let (counts, _trace) = retry_log("subscription_owner_retry");
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.runtime().await;
    runtime.reconcile(&fixture.reader).await;
    runtime.owner_barrier(&fixture.reader).await;
    while runtime.observations.lock().await.try_recv().is_ok() {}
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();

    for streak in 1..=2 {
        tokio::time::pause();
        let held = exhaust_project_readers(&project).await;
        runtime.reconcile(&fixture.reader).await;
        let last_deadline = walk_owner_backoff(&runtime, &fixture.clock).await;
        assert_eq!(
            logged(&counts, "failing"),
            streak,
            "one warning per run of failures, not per attempt"
        );
        // Recovery needs only the retry clock: no post, reconcile or store write.
        release_project_readers(held).await;
        tokio::time::resume();
        fixture
            .clock
            .advance(last_deadline.saturating_duration_since(fixture.clock.monotonic_now()))
            .unwrap();
        (async {
            while logged(&counts, "recovered") < streak {
                runtime
                    .observe(|event| matches!(event, OwnerObservation::Sleeping(_)))
                    .await;
            }
        })
        .await;
        while runtime.observations.lock().await.try_recv().is_ok() {}
    }
    runtime.close().await;
}

// SQLite works on background threads; the owner's retries run on the injected clock only.
#[tokio::test]
async fn a_failed_held_push_cleanup_backs_off_from_one_to_thirty_seconds_and_logs_once() {
    use super::f6_held_record_proofs::{assert_copied_snapshot_state, copied_held_snapshot};
    use collaboration_protocol::PushDeliveryState;
    use message_board::{
        SubscriptionMode, SubscriptionScope, ThreadSubscriptionUnsubscribeRequest, WhenIdle,
    };
    use sqlx::Connection;
    let (counts, _trace) = retry_log("held_cleanup_retry");
    let fixture = Box::pin(OwnerFixture::new()).await;
    fixture
        .policy(&fixture.root, SubscriptionMode::Poll, WhenIdle::Hold, 0, 0)
        .await;
    let position = fixture
        .post(&fixture.root, "Held before ending scope")
        .await;
    let held_id = copied_held_snapshot(&fixture, vec![(fixture.root.clone(), position)]).await;
    let runtime = fixture.runtime().await;
    runtime.reconcile(&fixture.reader).await;
    runtime.owner_barrier(&fixture.reader).await;
    while runtime.observations.lock().await.try_recv().is_ok() {}
    // The real settlement write fails at the store until the trigger is dropped.
    let mut observer = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    sqlx::raw_sql("CREATE TRIGGER fail_held_cleanup BEFORE UPDATE OF delivery_state ON router_pushes WHEN OLD.delivery_state='held' AND NEW.delivery_state='rejected' BEGIN SELECT RAISE(FAIL, 'injected cleanup write failure'); END").execute(&mut observer).await.unwrap();
    fixture
        .unsubscribe(
            ThreadSubscriptionUnsubscribeRequest {
                reader: fixture.reader.clone(),
                scope: SubscriptionScope::thread(fixture.root.clone()),
            },
            fixture.clock.utc_now(),
        )
        .await
        .unwrap();
    // The unsubscribe's own activity notice wakes the owner for the first attempt; any
    // other input would be a second pass, which also retries the clean-up.

    let last_deadline = walk_owner_backoff(&runtime, &fixture.clock).await;

    assert_eq!(
        logged(&counts, "failing"),
        1,
        "one warning per run of failures"
    );
    assert_copied_snapshot_state(&fixture, &held_id, PushDeliveryState::Held).await;
    // Recovery needs only the retry clock once the store accepts the write.
    sqlx::query("DROP TRIGGER fail_held_cleanup")
        .execute(&mut observer)
        .await
        .unwrap();
    fixture
        .clock
        .advance(last_deadline.saturating_duration_since(fixture.clock.monotonic_now()))
        .unwrap();
    (runtime.observe(|event| matches!(event, OwnerObservation::HeldSubscriptionPushesSettled(1))))
        .await;
    assert_copied_snapshot_state(&fixture, &held_id, PushDeliveryState::Rejected).await;
    assert_eq!(logged(&counts, "recovered"), 1);
    observer.close().await.unwrap();
    runtime.close().await;
}

#[tokio::test]
async fn deferred_startup_recovery_backs_off_from_one_to_thirty_seconds_and_logs_once() {
    use super::startup_fixture::{StartupEvidence, StartupTrace, activity, expected_range};
    use sqlx::Connection;
    let evidence = Arc::new(StartupEvidence::default());
    let _trace = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(StartupTrace(evidence.clone())),
    );
    let fixture = Box::pin(OwnerFixture::new()).await;
    let owed = fixture.post(&fixture.root, "owed before startup").await;
    let project = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    tokio::time::pause();
    let held = exhaust_project_readers(&project).await;
    let runtime = fixture.native_runtime().await;
    tokio::time::resume();
    evidence.state("waiting").await;
    // The task armed its first 1 s wait as it started; nothing has moved the clock since.
    let mut deadline = fixture.clock.monotonic_now() + Duration::from_secs(1);
    let mut deadlines = runtime.service.observe_startup_retry_deadlines();
    let mut passes = runtime.service.observe_startup_passes();
    // Every attempt now fails on the shared progress table, so none makes progress.
    let mut interactions = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
    )
    .await
    .unwrap();
    sqlx::query("ALTER TABLE subscription_delivery_progress RENAME TO unavailable_progress")
        .execute(&mut interactions)
        .await
        .unwrap();
    release_project_readers(held).await;

    let mut armed = Vec::new();
    for _ in BACKOFF_SECONDS {
        armed.push(deadline.saturating_duration_since(fixture.clock.monotonic_now()));
        let expected: Vec<Duration> = BACKOFF_SECONDS[..armed.len()]
            .iter()
            .map(|seconds| Duration::from_secs(*seconds))
            .collect();
        assert_eq!(armed, expected);
        step_to_deadline(&fixture.clock, deadline, passes.recv()).await;
        (passes.recv()).await.unwrap();
        deadline = (deadlines.recv()).await.unwrap();
    }

    let blocked = evidence
        .states
        .lock()
        .unwrap()
        .iter()
        .filter(|state| state.as_str() == "blocked")
        .count();
    assert_eq!(blocked, 1, "one warning per run of failures");
    // Recovery needs only the retry clock once the table is back.
    sqlx::query("ALTER TABLE unavailable_progress RENAME TO subscription_delivery_progress")
        .execute(&mut interactions)
        .await
        .unwrap();
    interactions.close().await.unwrap();
    fixture
        .clock
        .advance(deadline.saturating_duration_since(fixture.clock.monotonic_now()))
        .unwrap();
    let notice = activity(&fixture, &runtime).await;
    assert_eq!(
        notice.ranges,
        vec![expected_range(
            fixture.project.clone(),
            fixture.root.clone(),
            owed
        )]
    );
    runtime.await_settled().await;
    evidence.state("finished").await;
    runtime.close().await;
}
