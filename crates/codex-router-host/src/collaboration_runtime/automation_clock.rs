//! Custody deadlines and report timestamps use the Host's existing shared clock.
use collaboration_runtime::CentralAutomationClock;
use collaboration_service::SubscriptionClock;
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

pub(super) struct HostAutomationClock(pub Arc<dyn SubscriptionClock>);

impl CentralAutomationClock for HostAutomationClock {
    fn now_ms(&self) -> i64 {
        self.0.now().timestamp_millis()
    }

    fn wait_until(&self, deadline_ms: i64) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let delay = u64::try_from(deadline_ms.saturating_sub(self.now_ms())).unwrap_or(0);
        self.0
            .sleep_until(self.0.monotonic_now() + Duration::from_millis(delay))
    }
}
