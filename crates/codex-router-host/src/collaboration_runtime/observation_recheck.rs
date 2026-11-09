//! While lifecycle observation is unavailable, nothing appends the native `turn/completed`
//! a running scheduled Run waits for. Each observation retry instead re-checks once: the
//! scheduler steps its running native Runs, and readers re-check held Codex mail.
//!
//! Observation is unavailable when the native observer exited on a connection error (it
//! restarts with this backoff) or the lifecycle journal failed to open at start-up. A
//! failed open is not retried; it stays failed until the Host restarts, so its re-checks
//! continue on the capped backoff for the Host's lifetime.
use runtime_primitives::Backoff;
use std::time::Duration;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

/// The observation retry: 1 s doubling to 30 s.
pub(crate) const OBSERVATION_RETRY_BACKOFF: Backoff = Backoff {
    initial: Duration::from_secs(1),
    max: Duration::from_secs(30),
    factor: 2,
};

/// Moves once per observation retry; workers that wait on lifecycle observation wake on it.
#[derive(Clone, Debug)]
pub(crate) struct ObservationRechecks {
    retries: watch::Sender<u64>,
}

impl ObservationRechecks {
    pub(crate) fn new() -> Self {
        Self {
            retries: watch::channel(0).0,
        }
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.retries.subscribe()
    }

    /// One retry happened: everything waiting on observation re-checks once.
    pub(crate) fn recheck(&self) {
        self.retries
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }
}

/// Runs while the lifecycle journal is unavailable: one re-check per retry, on the
/// observation backoff, until shutdown.
pub(crate) async fn recheck_while_journal_unavailable(
    rechecks: ObservationRechecks,
    shutdown: CancellationToken,
) {
    let mut attempt = 0;
    loop {
        let delay = OBSERVATION_RETRY_BACKOFF.delay(attempt);
        tokio::select! {
            () = shutdown.cancelled() => return,
            () = tokio::time::sleep(delay) => {}
        }
        rechecks.recheck();
        attempt = attempt.saturating_add(1);
    }
}

/// How one observer attempt ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ObserverAttempt {
    /// It restored coverage and observed until the connection ended.
    Ran,
    /// It could not connect.
    ConnectFailed,
    /// It connected but stopped before restoring coverage, such as a failed inventory or
    /// journal write: observation never recovered, so this is a failed attempt too.
    StoppedBeforeCoverage,
    /// Its scope was rejected; retrying cannot help.
    Rejected,
}

/// What an observer attempt is told about the attempts before it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ObserverRetry {
    /// Any attempt after the first: observation was unavailable, so the attempt re-checks
    /// once it has connected.
    pub(crate) is_retry: bool,
    /// Failed attempts since the last one that restored coverage; the first failure of a
    /// run records lost coverage and warns once.
    pub(crate) failures_so_far: u32,
}

/// Runs the native observer until its generation retires, restarting it on the
/// observation backoff after it stops or cannot connect. Only an attempt that restored
/// coverage resets the backoff.
pub(crate) async fn observe_with_restart<TAttempt, TFuture>(
    retired: CancellationToken,
    mut attempt: TAttempt,
) where
    TAttempt: FnMut(ObserverRetry) -> TFuture,
    TFuture: std::future::Future<Output = ObserverAttempt>,
{
    let mut is_retry = false;
    let mut failures: u32 = 0;
    loop {
        let outcome = tokio::select! {
            () = retired.cancelled() => return,
            outcome = attempt(ObserverRetry { is_retry, failures_so_far: failures }) => outcome,
        };
        match outcome {
            ObserverAttempt::Ran => failures = 0,
            ObserverAttempt::ConnectFailed | ObserverAttempt::StoppedBeforeCoverage => {
                failures = failures.saturating_add(1);
            }
            ObserverAttempt::Rejected => return,
        }
        if retired.is_cancelled() {
            return;
        }
        // 1 s after a recovered observer ends or a run of failures starts, doubling across
        // failed attempts.
        let delay = OBSERVATION_RETRY_BACKOFF.delay(failures.saturating_sub(1));
        tracing::debug!(
            target: "native_observer",
            delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
            failures,
            "observer restarts after its backoff"
        );
        is_retry = true;
        tokio::select! {
            () = retired.cancelled() => return,
            () = tokio::time::sleep(delay) => {}
        }
    }
}

#[cfg(test)]
#[path = "observation_recheck_test.rs"]
mod tests;
