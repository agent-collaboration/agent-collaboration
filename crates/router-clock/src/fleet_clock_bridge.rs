//! The same clocks, read through FleetRuntime's clock port. Central crates use
//! that port; Router crates keep the agent-fleet-sync `Clock` until the Router
//! retrofit, so each clock answers both from one reading.
use crate::RuntimeClock;
use fleet_runtime_ports::{Clock, SyncTime};

fn fleet_now(clock: &impl RuntimeClock) -> SyncTime {
    SyncTime::from_unix_millis(clock.utc_now().timestamp_millis())
}

impl Clock for crate::SystemRuntimeClock {
    fn now(&self) -> SyncTime {
        fleet_now(self)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Clock for crate::ManualClock {
    fn now(&self) -> SyncTime {
        fleet_now(self)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Clock for crate::GatedClock {
    fn now(&self) -> SyncTime {
        fleet_now(self)
    }
}
