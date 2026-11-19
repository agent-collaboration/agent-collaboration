//! Provider load veto for a session currently owned by another live client.
use collaboration_contracts::DeliveryFuture;
use collaboration_protocol::HarnessSessionRef;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveSessionOwnership {
    NotLive,
    LiveWritable,
    LiveUnsupported,
}

pub trait LiveSessionOwnershipCheck: Send + Sync {
    fn check<'a>(
        &'a self,
        target: &'a HarnessSessionRef,
    ) -> DeliveryFuture<'a, LiveSessionOwnership>;
}
