//! Permanent proof checkpoints around the real external waiter handover.
use super::*;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::service_identity::subscription_delivery) enum PollHandoverStage {
    BeforeSend,
    AfterSend,
}

pub(in crate::service_identity::subscription_delivery) struct PollHandoverPause {
    pub push: PushId,
    pub stage: PollHandoverStage,
    pub entered: oneshot::Sender<()>,
    pub resume: oneshot::Receiver<()>,
}

pub(in crate::service_identity::subscription_delivery) type PollHandoverProbeSlot =
    Arc<std::sync::Mutex<Option<PollHandoverPause>>>;

pub(in crate::service_identity::subscription_delivery) struct PollHandoverWitness {
    pub entered: oneshot::Receiver<()>,
    pub resume: oneshot::Sender<()>,
}

impl SubscriptionDeliveryService {
    pub(in crate::service_identity::subscription_delivery) fn pause_poll_handover_for_test(
        &self,
        push: PushId,
        stage: PollHandoverStage,
    ) -> PollHandoverWitness {
        let (entered, observed) = oneshot::channel();
        let (resume, waiting) = oneshot::channel();
        assert!(
            self.inner
                .poll_handover_probe
                .lock()
                .unwrap()
                .replace(PollHandoverPause {
                    push,
                    stage,
                    entered,
                    resume: waiting,
                })
                .is_none()
        );
        PollHandoverWitness {
            entered: observed,
            resume,
        }
    }
}
