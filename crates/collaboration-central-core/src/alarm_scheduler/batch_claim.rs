//! The cadence claim: at most one due batch per object per cadence interval.
//!
//! Nothing here is stored. A crash or a nap drops the claim with the object's
//! memory, and the next event may run one extra batch, which is harmless. The
//! check and the set happen under one synchronous lock with no await between
//! them, so two events can't both claim a batch: on Cloudflare another event
//! only runs at an await, and on Tokio the lock serializes them.

use runtime_primitives::SyncTime;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// Two due batches never start closer together than this while the object stays in memory.
pub(crate) const CADENCE: Duration = Duration::from_secs(30);
/// One batch stops after this long and leaves the rest for the next batch.
pub(crate) const BATCH_DEADLINE: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ClaimState {
    last_batch_at: Option<SyncTime>,
    running_since: Option<SyncTime>,
}

#[derive(Debug, Default)]
pub(crate) struct BatchClaim {
    state: Mutex<ClaimState>,
}

/// Why an event did not run the due batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClaimDeclined {
    /// Another event's batch is running.
    Running,
    /// A batch started less than one cadence ago; the next may start at `until`.
    Cadence { until: SyncTime },
}

/// Held for the whole batch; dropping it releases the claim, including on error.
#[derive(Debug)]
pub(crate) struct ClaimGuard<'claim> {
    claim: &'claim BatchClaim,
}

impl BatchClaim {
    pub(crate) fn try_claim(&self, now: SyncTime) -> Result<ClaimGuard<'_>, ClaimDeclined> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(since) = state.running_since {
            if now < stale_after(since) {
                return Err(ClaimDeclined::Running);
            }
            // The batch is bounded by BATCH_DEADLINE, so a claim this old is a defect.
            eprintln!(
                "central batch claim from {since:?} outlived its deadline; treating it as free"
            );
        }
        if let Some(until) = state.last_batch_at.map(cadence_end)
            && now < until
        {
            return Err(ClaimDeclined::Cadence { until });
        }
        state.running_since = Some(now);
        state.last_batch_at = Some(now);
        Ok(ClaimGuard { claim: self })
    }

    /// The earliest time the next batch may start, if a batch ran in this instance.
    pub(crate) fn next_batch_at(&self) -> Option<SyncTime> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.last_batch_at.map(cadence_end)
    }
}

impl Drop for ClaimGuard<'_> {
    fn drop(&mut self) {
        let mut state = self
            .claim
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.running_since = None;
    }
}

fn cadence_end(start: SyncTime) -> SyncTime {
    start.after(CADENCE)
}

fn stale_after(since: SyncTime) -> SyncTime {
    since.after(BATCH_DEADLINE.saturating_mul(2))
}

#[cfg(test)]
#[path = "batch_claim_test.rs"]
mod tests;
