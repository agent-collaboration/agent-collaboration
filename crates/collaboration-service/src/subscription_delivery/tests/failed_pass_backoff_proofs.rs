//! A failed owner pass, and a failed held-push clean-up, retry on a 1 s to 30 s backoff
//! and log once per run of failures.
use super::super::{SubscriptionClock, subscription_service::OwnerObservation};
use super::owner_fixture::*;
use super::startup_fixture::{exhaust_project_readers, release_project_readers};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use tracing_subscriber::{Layer, layer::SubscriberExt};

const BACKOFF_SECONDS: [u64; 7] = [1, 2, 4, 8, 16, 30, 30];

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

/// The armed delay of the owner's next sleep, read against the injected monotonic clock.
/// The bound only stops a hang; it outlasts the paused test's storage acquire timeout.
async fn next_armed_delay(runtime: &OwnerRuntime, clock: &TestClock) -> Duration {
    tokio::time::timeout(Duration::from_secs(60), async {
        let mut events = runtime.observations.lock().await;
        loop {
            if let OwnerObservation::Sleeping(Some(deadline)) = events.recv().await.unwrap() {
                return deadline.duration_since(clock.monotonic_now());
            }
        }
    })
    .await
    .expect("the owner armed no deadline after its pass")
}

#[tokio::test(start_paused = true)]
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
        let held = exhaust_project_readers(&project).await;
        runtime.reconcile(&fixture.reader).await;
        let mut armed = Vec::new();
        for expected in BACKOFF_SECONDS {
            armed.push(next_armed_delay(&runtime, &fixture.clock).await.as_secs());
            assert_eq!(armed, BACKOFF_SECONDS[..armed.len()], "streak {streak}");
            fixture.clock.advance_without_tokio_time(expected);
        }
        assert_eq!(
            armed,
            BACKOFF_SECONDS.to_vec(),
            "each failed pass waits the next backoff step (streak {streak})"
        );
        assert_eq!(
            logged(&counts, "failing"),
            streak,
            "one warning per run of failures, not per attempt"
        );
        // Recovery needs only the retry clock: no post, reconcile or store write.
        release_project_readers(held).await;
        fixture.clock.advance_without_tokio_time(30);
        tokio::time::timeout(Duration::from_secs(60), async {
            while logged(&counts, "recovered") < streak {
                runtime
                    .observe(|event| matches!(event, OwnerObservation::Sleeping(_)))
                    .await;
            }
        })
        .await
        .expect("the owner pass recovered once its storage did");
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
            fixture.clock.now(),
        )
        .await
        .unwrap();
    runtime.reconcile(&fixture.reader).await;

    let mut armed = Vec::new();
    for expected in BACKOFF_SECONDS {
        armed.push(next_armed_delay(&runtime, &fixture.clock).await.as_secs());
        assert_eq!(armed, BACKOFF_SECONDS[..armed.len()]);
        fixture.clock.advance_without_tokio_time(expected);
    }

    assert_eq!(armed, BACKOFF_SECONDS.to_vec());
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
    fixture.clock.advance_without_tokio_time(30);
    tokio::time::timeout(
        Duration::from_secs(5),
        runtime
            .observe(|event| matches!(event, OwnerObservation::HeldSubscriptionPushesSettled(1))),
    )
    .await
    .expect("the retried clean-up settled the held push");
    assert_copied_snapshot_state(&fixture, &held_id, PushDeliveryState::Rejected).await;
    assert_eq!(logged(&counts, "recovered"), 1);
    observer.close().await.unwrap();
    runtime.close().await;
}

#[tokio::test(start_paused = true)]
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
    let held = exhaust_project_readers(&project).await;
    let runtime = fixture.native_runtime().await;
    tokio::time::resume();
    evidence.state("waiting").await;
    let mut delays = runtime.service.observe_startup_retry_delays();
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

    fixture.clock.advance_without_tokio_time(1);
    let mut armed = Vec::new();
    for expected in &BACKOFF_SECONDS[1..] {
        let delay = tokio::time::timeout(Duration::from_secs(5), delays.recv())
            .await
            .expect("the startup task armed no next wait")
            .unwrap();
        armed.push(delay.as_secs());
        assert_eq!(armed, BACKOFF_SECONDS[1..=armed.len()]);
        fixture.clock.advance_without_tokio_time(*expected);
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
    fixture.clock.advance_without_tokio_time(30);
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
