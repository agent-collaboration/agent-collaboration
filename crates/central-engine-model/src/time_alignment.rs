//! Shared UTC grid arithmetic for due and cadence choices.
use crate::{EngineTime, supported_time};
use chrono::Duration;

const GRID_SECONDS: u32 = 300;

pub fn aligned_at_or_after(time: EngineTime, seconds: u32) -> Option<EngineTime> {
    if seconds == 0 {
        return None;
    }
    let seconds = i64::from(seconds);
    let remainder = time.timestamp().rem_euclid(seconds);
    let add = if remainder == 0 && time.timestamp_subsec_nanos() == 0 {
        0
    } else {
        seconds - remainder
    };
    let whole = time.timestamp().checked_add(add)?;
    EngineTime::from_timestamp(whole, 0).filter(|value| supported_time(*value))
}

pub fn aligned_strictly_after(time: EngineTime, seconds: u32) -> Option<EngineTime> {
    let after = time.checked_add_signed(Duration::nanoseconds(1))?;
    aligned_at_or_after(after, seconds)
}

pub fn on_five_minute_grid(time: EngineTime) -> bool {
    time.timestamp().rem_euclid(i64::from(GRID_SECONDS)) == 0 && time.timestamp_subsec_nanos() == 0
}

#[cfg(test)]
#[path = "time_alignment_test.rs"]
mod tests;
