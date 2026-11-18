use super::ClockHandle;
use fleet_runtime_ports::{Clock, SyncTime};
use router_clock::{ClockInstant, RuntimeClock};
use std::{future::Future, pin::Pin, sync::Arc};

struct DistinctClockViews;

impl runtime_primitives::Clock for DistinctClockViews {
    fn now(&self) -> runtime_primitives::SyncTime {
        runtime_primitives::SyncTime::from_unix_millis(1_800_000_000_123)
    }
}

impl RuntimeClock for DistinctClockViews {
    fn utc_now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp_millis(1_800_000_001_234).unwrap()
    }

    fn monotonic_now(&self) -> ClockInstant {
        ClockInstant::ZERO
    }

    fn sleep_until(&self, _: ClockInstant) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(std::future::pending())
    }
}

#[test]
fn the_fleet_clock_preserves_the_injected_now_dispatch() {
    let clock = ClockHandle(Arc::new(DistinctClockViews));
    assert_eq!(clock.now(), SyncTime::from_unix_millis(1_800_000_000_123));
}
