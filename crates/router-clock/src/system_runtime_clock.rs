use crate::{ClockInstant, RuntimeClock};
use chrono::{DateTime, Utc};
use runtime_primitives::{Clock, SyncTime};
use std::{future::Future, pin::Pin, time::Instant};

/// Native time access is confined to this implementation.
pub struct SystemRuntimeClock {
    started: Instant,
}

impl SystemRuntimeClock {
    #[must_use]
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl Default for SystemRuntimeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemRuntimeClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.utc_now().timestamp_millis())
    }
}

impl RuntimeClock for SystemRuntimeClock {
    fn utc_now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn monotonic_now(&self) -> ClockInstant {
        ClockInstant(self.started.elapsed())
    }

    fn sleep_until(&self, deadline: ClockInstant) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            tokio::time::sleep(deadline.saturating_duration_since(self.monotonic_now())).await;
        })
    }
}
