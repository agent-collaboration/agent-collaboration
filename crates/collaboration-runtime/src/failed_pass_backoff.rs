//! The retry every converted worker and subscription retry uses after a failed attempt:
//! 1 s doubling to 30 s, with only the failure that starts a run logged.
use runtime_primitives::Backoff;
use std::time::Duration;

/// 1 s, then 2, 4, 8, 16 and 30 s for every later failure in the run.
pub const FAILED_PASS_BACKOFF: Backoff = Backoff {
    initial: Duration::from_secs(1),
    max: Duration::from_secs(30),
    factor: 2,
};

/// Counts one run of consecutive failures; a success ends the run.
#[derive(Debug, Default)]
pub struct FailedPassBackoff {
    consecutive_failures: u32,
}

/// What the caller does after one more failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FailureRetry {
    /// How long to wait before the next attempt.
    pub delay: Duration,
    /// True only for the failure that starts a run, which is the one the caller logs.
    pub starts_run: bool,
}

impl FailedPassBackoff {
    pub fn record_failure(&mut self) -> FailureRetry {
        let retry = FailureRetry {
            delay: FAILED_PASS_BACKOFF.delay(self.consecutive_failures),
            starts_run: self.consecutive_failures == 0,
        };
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        retry
    }

    /// Returns true when this success ends a run of failures, so the caller logs recovery once.
    pub fn record_success(&mut self) -> bool {
        std::mem::take(&mut self.consecutive_failures) > 0
    }
}

#[cfg(test)]
#[path = "failed_pass_backoff_test.rs"]
mod tests;
