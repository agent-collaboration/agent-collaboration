//! One opt-in debug setting composes the notification clock and its owner-socket controller.
use collaboration_service::DebugSubscriptionClock;
use std::{io, sync::Arc};

pub(super) fn configured_clock() -> io::Result<Option<Arc<DebugSubscriptionClock>>> {
    match std::env::var("CODEX_ROUTER_DEBUG_NOTIFICATION_CLOCK_MS") {
        Err(std::env::VarError::NotPresent) => Ok(None),
        Ok(value) => {
            let millis = value.parse::<i64>().map_err(|_| {
                io::Error::other(
                    "CODEX_ROUTER_DEBUG_NOTIFICATION_CLOCK_MS must be UTC epoch milliseconds",
                )
            })?;
            let now = chrono::DateTime::from_timestamp_millis(millis).ok_or_else(|| {
                io::Error::other("debug notification clock is outside the UTC timestamp range")
            })?;
            Ok(Some(Arc::new(DebugSubscriptionClock::new(now))))
        }
        Err(std::env::VarError::NotUnicode(_)) => Err(io::Error::other(
            "debug notification clock setting is not UTF-8",
        )),
    }
}
