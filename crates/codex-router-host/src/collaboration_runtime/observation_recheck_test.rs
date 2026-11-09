use super::{ObservationRechecks, recheck_while_journal_unavailable};
use router_clock::{ClockInstant, ManualClock, RuntimeClock};
use std::{future::Future, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

fn without_native_time(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(test);
}

fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new("2001-01-01T00:00:00Z".parse().unwrap()))
}

async fn advance(clock: &ManualClock, by: Duration) {
    clock.advance(by).unwrap();
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
}

#[test]
fn an_unavailable_journal_rechecks_at_one_two_four_seconds_up_to_thirty() {
    without_native_time(async {
        let clock = clock();
        let rechecks = ObservationRechecks::new();
        let mut retries = rechecks.subscribe();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(recheck_while_journal_unavailable(
            rechecks.clone(),
            clock.clone(),
            shutdown.clone(),
        ));
        advance(&clock, Duration::ZERO).await;
        assert!(clock.has_not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(1))));
        let mut gaps = Vec::new();

        for _ in 0..7 {
            let mut waited = 0;
            while !retries.has_changed().unwrap() {
                assert!(
                    !task.is_finished(),
                    "journal retry task stopped before its injected wait"
                );
                advance(&clock, Duration::from_millis(500)).await;
                waited += 500;
            }
            retries.borrow_and_update();
            gaps.push(waited);
        }
        shutdown.cancel();
        task.await.unwrap();

        assert_eq!(gaps, [1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000]);
    });
}

#[test]
fn shutdown_stops_the_rechecks() {
    without_native_time(async {
        let clock = clock();
        let rechecks = ObservationRechecks::new();
        let retries = rechecks.subscribe();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(recheck_while_journal_unavailable(
            rechecks.clone(),
            clock.clone(),
            shutdown.clone(),
        ));

        shutdown.cancel();
        task.await.unwrap();
        advance(&clock, Duration::from_secs(60)).await;

        assert!(!retries.has_changed().unwrap());
    });
}

/// Each attempt's start time, in injected elapsed seconds, and what it was told.
type AttemptLog = Arc<std::sync::Mutex<Vec<(u64, super::ObserverRetry)>>>;

async fn run_observer(
    outcomes: Vec<super::ObserverAttempt>,
    retire_after: Duration,
) -> Vec<(u64, super::ObserverRetry)> {
    let clock = clock();
    let started = clock.monotonic_now();
    let log = AttemptLog::default();
    let retired = CancellationToken::new();
    let attempts = Arc::new(std::sync::Mutex::new(outcomes.into_iter()));
    let task = tokio::spawn({
        let (log, retired, attempts) = (log.clone(), retired.clone(), attempts.clone());
        let clock = clock.clone();
        async move {
            let observation_clock = clock.clone();
            super::observe_with_restart(retired, clock, move |retry| {
                log.lock().unwrap().push((
                    observation_clock
                        .monotonic_now()
                        .saturating_duration_since(started)
                        .as_secs(),
                    retry,
                ));
                let next = attempts.lock().unwrap().next();
                async move {
                    match next {
                        Some(outcome) => outcome,
                        None => std::future::pending().await,
                    }
                }
            })
            .await;
        }
    });
    advance(&clock, Duration::ZERO).await;
    let mut elapsed = Duration::ZERO;
    while elapsed < retire_after {
        advance(&clock, Duration::from_millis(500)).await;
        elapsed += Duration::from_millis(500);
    }
    retired.cancel();
    task.await.unwrap();
    log.lock().unwrap().clone()
}

fn retry(is_retry: bool, failures_so_far: u32) -> super::ObserverRetry {
    super::ObserverRetry {
        is_retry,
        failures_so_far,
    }
}

#[test]
fn the_observer_restarts_on_the_backoff_and_a_run_resets_it() {
    without_native_time(async {
        use super::ObserverAttempt::{ConnectFailed, Ran};
        let attempts = run_observer(
            vec![
                ConnectFailed,
                ConnectFailed,
                ConnectFailed,
                Ran,
                ConnectFailed,
            ],
            Duration::from_secs(20),
        )
        .await;
        assert_eq!(
            attempts,
            [
                (0, retry(false, 0)),
                (1, retry(true, 1)),
                (3, retry(true, 2)),
                (7, retry(true, 3)),
                (8, retry(true, 0)),
                (9, retry(true, 1)),
            ]
        );
    });
}

#[test]
fn a_rejected_scope_stops_the_observer_for_good() {
    without_native_time(async {
        let attempts = run_observer(
            vec![super::ObserverAttempt::Rejected],
            Duration::from_secs(60),
        )
        .await;
        assert_eq!(attempts, [(0, retry(false, 0))]);
    });
}
