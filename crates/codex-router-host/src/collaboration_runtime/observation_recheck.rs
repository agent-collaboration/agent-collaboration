//! The native observer restarts on the observation backoff after it stops or cannot connect.
use router_clock::RuntimeClock;
use runtime_primitives::Backoff;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// The observation retry: 1 s doubling to 30 s.
pub(crate) const OBSERVATION_RETRY_BACKOFF: Backoff = Backoff {
    initial: Duration::from_secs(1),
    max: Duration::from_secs(30),
    factor: 2,
};

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
    clock: Arc<dyn RuntimeClock>,
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
            () = clock.sleep_for(delay) => {}
        }
    }
}

#[cfg(test)]
#[path = "observation_recheck_test.rs"]
mod tests;
