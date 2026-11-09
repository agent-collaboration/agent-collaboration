//! Sized read-only view required by the inherited SDK mail host.
use router_clock::RuntimeClock;

pub(crate) struct SubscriptionRuntimeClock(pub(crate) std::sync::Arc<dyn RuntimeClock>);

impl runtime_primitives::Clock for SubscriptionRuntimeClock {
    fn now(&self) -> runtime_primitives::SyncTime {
        self.0.now()
    }
}
