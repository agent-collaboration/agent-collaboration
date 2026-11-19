//! When central cancels a direct message nobody delivered (owner, 2026-11-17,
//! record:358 and the delivery design): one 15 minutes, counted from when the
//! agent sent it. The cancel is a due item on the 5-minute grid, run at the
//! first mark at least 2 minutes after the 15 minutes end, so an Ack riding
//! the recipient's minute flush (up to ~63 s) and a due batch running up to
//! 50 s early can never report a delivered message as cancelled.
use chrono::{DateTime, Duration, Utc};

/// The one delivery window, from when the agent sent the message.
pub const DELIVERY_WINDOW: Duration = Duration::minutes(15);
pub(crate) const SYSTEM_MESSAGE_DELIVERY_WINDOW: Duration = Duration::hours(24);
/// Room for the recipient's Ack before the cancel runs.
pub const ACK_MARGIN: Duration = Duration::minutes(2);
/// Due items run on this grid, like wakes.
const DUE_GRID_SECONDS: i64 = 5 * 60;

/// The cancel's 5-minute mark for a direct message the agent sent at
/// `sent_at` (the Mac's clock) and central accepted at `accepted_at`.
/// `sent_at` is clamped into [accepted − 15 min, accepted]: a fast Mac clock
/// cannot stretch the window, and anything older the Mac has already
/// cancelled. Add 15 minutes and the 2-minute margin, then round up to the
/// next 5-minute mark (a time already on a mark stays).
#[must_use]
pub fn cancel_due_at(sent_at: DateTime<Utc>, accepted_at: DateTime<Utc>) -> DateTime<Utc> {
    round_up_to_grid(delivery_window_ends_at(sent_at, accepted_at) + ACK_MARGIN)
}

pub(crate) fn delivery_window_ends_at(
    sent_at: DateTime<Utc>,
    accepted_at: DateTime<Utc>,
) -> DateTime<Utc> {
    sent_at.clamp(accepted_at - DELIVERY_WINDOW, accepted_at) + DELIVERY_WINDOW
}

/// Whether the message's delivery window has run out at `now`: central never
/// pushes it after this, not even on reconnect.
#[must_use]
pub fn delivery_window_open(
    sent_at: DateTime<Utc>,
    accepted_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    now < delivery_window_ends_at(sent_at, accepted_at)
}

fn round_up_to_grid(at: DateTime<Utc>) -> DateTime<Utc> {
    let millis = at.timestamp_millis();
    let grid = DUE_GRID_SECONDS * 1000;
    let rounded = millis.div_euclid(grid) * grid
        + if millis.rem_euclid(grid) == 0 {
            0
        } else {
            grid
        };
    DateTime::from_timestamp_millis(rounded).unwrap_or(at)
}

#[cfg(test)]
#[path = "message_deadline_test.rs"]
mod tests;
