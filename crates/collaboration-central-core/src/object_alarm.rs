//! The object's one alarm: units lower it, only the recompute sets it exactly.
//!
//! Lowering is `WakeScheduler::wake_at` and never postpones an earlier wake, so a
//! unit can commit accepted work together with its wake. Exact writes replace or
//! delete the alarm and belong to the scheduler's serialized recompute and to
//! repair alone; nothing else may move the alarm later.

use runtime_primitives::{SyncTime, WakeFailure, WakeScheduler};
use std::future::Future;

pub trait ObjectAlarm: WakeScheduler + 'static {
    /// Replace the stored alarm with `at`, or delete it when `None`.
    fn set_exact(
        &self,
        at: Option<SyncTime>,
    ) -> impl Future<Output = Result<(), WakeFailure>> + Send;

    /// Read back the alarm the platform will deliver next.
    fn stored(&self) -> impl Future<Output = Result<Option<SyncTime>, WakeFailure>> + Send;
}
