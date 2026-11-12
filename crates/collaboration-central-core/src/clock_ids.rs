//! UUIDv7 ids minted from the injected clock, never the system clock.
//!
//! Ids are identity only; order comes from change-log positions. One `ClockIds` per
//! object keeps a `ContextV7`, so ids minted in the same millisecond (or while a test
//! holds the clock) still increase in mint order within that object.

use runtime_primitives::{Clock, IdSource, SyncTime};
use std::sync::{Mutex, PoisonError};
use uuid::{ContextV7, Timestamp, Uuid};

pub struct ClockIds<C: Clock> {
    clock: C,
    context: Mutex<ContextV7>,
}

impl<C: Clock> ClockIds<C> {
    pub fn new(clock: C) -> Self {
        Self {
            clock,
            context: Mutex::new(ContextV7::new()),
        }
    }
}

impl<C: Clock> IdSource for ClockIds<C> {
    fn new_id(&self) -> Uuid {
        let (seconds, subsec_nanos) = unix_parts(self.clock.now());
        let context = self.context.lock().unwrap_or_else(PoisonError::into_inner);
        Uuid::new_v7(Timestamp::from_unix(&*context, seconds, subsec_nanos))
    }
}

/// Splits a clock reading into Unix seconds and nanoseconds; times before the epoch mint at it.
fn unix_parts(now: SyncTime) -> (u64, u32) {
    let milliseconds = u64::try_from(now.unix_millis()).unwrap_or(0);
    let subsec_millis = u32::try_from(milliseconds % 1_000).unwrap_or(0);
    (milliseconds / 1_000, subsec_millis * 1_000_000)
}

#[cfg(test)]
#[path = "clock_ids_test.rs"]
mod tests;
