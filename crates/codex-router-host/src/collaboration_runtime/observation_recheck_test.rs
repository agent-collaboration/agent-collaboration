use super::{ObservationRechecks, recheck_while_journal_unavailable};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Advances Tokio's paused clock and lets the re-check task run.
async fn advance(by: Duration) {
    tokio::time::advance(by).await;
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn an_unavailable_journal_rechecks_at_one_two_four_seconds_up_to_thirty() {
    let rechecks = ObservationRechecks::new();
    let mut retries = rechecks.subscribe();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(recheck_while_journal_unavailable(
        rechecks.clone(),
        shutdown.clone(),
    ));
    // Let the task start and arm its first wait before the clock moves.
    advance(Duration::ZERO).await;
    let mut gaps = Vec::new();

    for _ in 0..7 {
        let mut waited = 0;
        while !retries.has_changed().unwrap() {
            advance(Duration::from_millis(500)).await;
            waited += 500;
        }
        retries.borrow_and_update();
        gaps.push(waited);
    }
    shutdown.cancel();
    task.await.unwrap();

    assert_eq!(gaps, [1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000]);
}

#[tokio::test(start_paused = true)]
async fn shutdown_stops_the_rechecks() {
    let rechecks = ObservationRechecks::new();
    let retries = rechecks.subscribe();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(recheck_while_journal_unavailable(
        rechecks.clone(),
        shutdown.clone(),
    ));

    shutdown.cancel();
    task.await.unwrap();
    advance(Duration::from_secs(60)).await;

    assert!(!retries.has_changed().unwrap());
}

/// Each attempt's start time, as seconds after the observer started, and what it was told.
type AttemptLog = std::sync::Arc<std::sync::Mutex<Vec<(u64, super::ObserverRetry)>>>;

async fn run_observer(
    outcomes: Vec<super::ObserverAttempt>,
    retire_after: Duration,
) -> Vec<(u64, super::ObserverRetry)> {
    let started = tokio::time::Instant::now();
    let log = AttemptLog::default();
    let retired = CancellationToken::new();
    let attempts = std::sync::Arc::new(std::sync::Mutex::new(outcomes.into_iter()));
    let task = tokio::spawn({
        let (log, retired, attempts) = (log.clone(), retired.clone(), attempts.clone());
        async move {
            super::observe_with_restart(retired, move |retry| {
                log.lock()
                    .unwrap()
                    .push((started.elapsed().as_secs(), retry));
                let next = attempts.lock().unwrap().next();
                async move {
                    match next {
                        Some(outcome) => outcome,
                        // Out of script: stay connected until retired.
                        None => std::future::pending().await,
                    }
                }
            })
            .await;
        }
    });
    advance(Duration::ZERO).await;
    let mut elapsed = Duration::ZERO;
    while elapsed < retire_after {
        advance(Duration::from_millis(500)).await;
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

#[tokio::test(start_paused = true)]
async fn the_observer_restarts_on_the_backoff_and_a_run_resets_it() {
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
            // The connection that ran resets the backoff.
            (8, retry(true, 0)),
            (9, retry(true, 1)),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_rejected_scope_stops_the_observer_for_good() {
    let attempts = run_observer(
        vec![super::ObserverAttempt::Rejected],
        Duration::from_secs(60),
    )
    .await;

    assert_eq!(attempts, [(0, retry(false, 0))]);
}
