use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::time::Instant;

/// Wall time carries durable deadlines; monotonic time drives the live owner.
pub trait SubscriptionClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
    fn monotonic_now(&self) -> Instant;
    fn sleep_until(
        &self,
        deadline: Instant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep_until(deadline))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemSubscriptionClock;

impl SubscriptionClock for SystemSubscriptionClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
    fn monotonic_now(&self) -> Instant {
        Instant::now()
    }
}

/// Share one subscription clock with runtime workers without reversing the crate dependency.
pub struct SubscriptionRuntimeClock(Arc<dyn SubscriptionClock>);

impl SubscriptionRuntimeClock {
    pub fn new(clock: Arc<dyn SubscriptionClock>) -> Self {
        Self(clock)
    }
}

impl runtime_primitives::Clock for SubscriptionRuntimeClock {
    fn now(&self) -> runtime_primitives::SyncTime {
        runtime_primitives::SyncTime::from_unix_millis(self.0.now().timestamp_millis())
    }
}
