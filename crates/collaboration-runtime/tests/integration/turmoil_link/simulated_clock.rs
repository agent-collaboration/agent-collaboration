//! The one injected clock inside a Turmoil simulation: each simulated host owns
//! one. Turmoil runs every host on a paused current-thread Tokio runtime and
//! advances it one tick per `Sim::step`, so `turmoil::elapsed`,
//! `turmoil::since_epoch` and Tokio sleeps read the same simulated time. No
//! native clock is read and no native timer is armed.
use chrono::{DateTime, Utc};
use router_clock::{ClockInstant, RuntimeClock};
use runtime_primitives::{Clock, SyncTime};
use std::{future::Future, pin::Pin};

pub(super) struct SimulatedClock {
    host_name: &'static str,
}

impl SimulatedClock {
    /// Create inside the host's software: Turmoil answers for the running host.
    pub(super) fn for_host(host_name: &'static str) -> Self {
        Self { host_name }
    }
}

impl Clock for SimulatedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.utc_now().timestamp_millis())
    }
}

impl RuntimeClock for SimulatedClock {
    fn utc_now(&self) -> DateTime<Utc> {
        #[expect(
            clippy::expect_used,
            reason = "a simulated clock read outside its simulated host is a harness defect"
        )]
        let since_epoch = turmoil::since_epoch().expect(self.host_name);
        DateTime::UNIX_EPOCH + since_epoch
    }

    fn monotonic_now(&self) -> ClockInstant {
        ClockInstant::ZERO.after(turmoil::elapsed())
    }

    fn sleep_until(&self, deadline: ClockInstant) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            tokio::time::sleep(deadline.saturating_duration_since(self.monotonic_now())).await;
        })
    }
}
