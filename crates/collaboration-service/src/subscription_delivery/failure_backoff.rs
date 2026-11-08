//! A run of failed attempts backs off from 1 s to 30 s and is logged once, at its start.
use std::time::Duration;

const FIRST_RETRY_DELAY: Duration = Duration::from_secs(1);
const LONGEST_RETRY_DELAY: Duration = Duration::from_secs(30);

/// Counts one run of consecutive failures; a success ends the run.
#[derive(Debug, Default)]
pub(super) struct FailureBackoff {
    consecutive_failures: u32,
}

/// What the caller does after one more failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FailureRetry {
    /// How long to wait before the next attempt: 1, 2, 4, 8, 16, then 30 s.
    pub delay: Duration,
    /// True only for the failure that starts a run, which is the one the caller logs.
    pub starts_run: bool,
}

impl FailureBackoff {
    pub(super) fn record_failure(&mut self) -> FailureRetry {
        let doublings = self.consecutive_failures.min(5);
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        FailureRetry {
            delay: FIRST_RETRY_DELAY
                .saturating_mul(1_u32 << doublings)
                .min(LONGEST_RETRY_DELAY),
            starts_run: self.consecutive_failures == 1,
        }
    }

    /// Returns true when this success ends a run of failures, so the caller logs recovery once.
    pub(super) fn record_success(&mut self) -> bool {
        std::mem::take(&mut self.consecutive_failures) > 0
    }
}

#[cfg(test)]
#[path = "failure_backoff_test.rs"]
mod tests;
