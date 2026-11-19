//! Permanent proof checkpoints around the real external waiter handover.
use super::*;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::subscription_delivery) enum PollHandoverStage {
    BeforeSend,
    AfterSend,
}

pub(in crate::subscription_delivery) struct PollHandoverPause {
    pub push: PushId,
    pub stage: PollHandoverStage,
    pub entered: oneshot::Sender<()>,
    pub resume: oneshot::Receiver<()>,
}

pub(in crate::subscription_delivery) type PollHandoverProbeSlot =
    Arc<std::sync::Mutex<Option<PollHandoverPause>>>;

impl SubscriptionDeliveryService {}
