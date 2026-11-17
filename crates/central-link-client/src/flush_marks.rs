//! The shared minute grid the outbox flushes on (owner 2026-11-15: Routers send
//! on 1-minute marks with ±3 s jitter).
use runtime_primitives::SyncTime;

pub const FLUSH_INTERVAL_MILLIS: i64 = 60_000;
pub const FLUSH_JITTER_MILLIS: i64 = 3_000;

/// The first 1-minute mark strictly after `now`.
#[must_use]
pub fn next_mark_after(now: SyncTime) -> SyncTime {
    let next_index = now
        .unix_millis()
        .div_euclid(FLUSH_INTERVAL_MILLIS)
        .saturating_add(1);
    SyncTime::from_unix_millis(next_index.saturating_mul(FLUSH_INTERVAL_MILLIS))
}

/// The mark whose ±3 s window holds `now`, if any.
#[must_use]
pub fn mark_in_window(now: SyncTime) -> Option<SyncTime> {
    let millis = now.unix_millis();
    let nearest_index = millis
        .saturating_add(FLUSH_INTERVAL_MILLIS / 2)
        .div_euclid(FLUSH_INTERVAL_MILLIS);
    let nearest = nearest_index.saturating_mul(FLUSH_INTERVAL_MILLIS);
    (millis.abs_diff(nearest) <= FLUSH_JITTER_MILLIS.unsigned_abs())
        .then_some(SyncTime::from_unix_millis(nearest))
}

/// The earliest mark a write stored at `now` can still make: a mark whose
/// window has not closed and that has not already flushed.
#[must_use]
pub fn earliest_open_mark(now: SyncTime, last_flushed: Option<SyncTime>) -> SyncTime {
    let window_open = mark_in_window(now).unwrap_or_else(|| next_mark_after(now));
    match last_flushed {
        Some(flushed) if flushed >= window_open => next_mark_after(flushed),
        Some(_) | None => window_open,
    }
}

#[cfg(test)]
#[path = "flush_marks_test.rs"]
mod tests;
