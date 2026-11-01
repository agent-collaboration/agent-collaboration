//! Causal test checkpoint before an idle reader closes admission and drains accepted work.
use super::*;

pub(in crate::subscription_delivery) struct IdleOwnerClosePause {
    entered: oneshot::Sender<()>,
    resume: oneshot::Receiver<()>,
}
pub(in crate::subscription_delivery) type IdleOwnerCloseProbeSlot =
    Arc<std::sync::Mutex<Option<IdleOwnerClosePause>>>;

pub(in crate::subscription_delivery) async fn pause_before_close(slot: &IdleOwnerCloseProbeSlot) {
    let pause = slot.lock().unwrap().take();
    if let Some(pause) = pause {
        pause
            .entered
            .send(())
            .expect("idle owner observer attached");
        pause.resume.await.expect("idle owner explicitly released");
    }
}
impl SubscriptionDeliveryService {
    pub(in crate::subscription_delivery) fn pause_idle_owner_close_for_test(
        &self,
    ) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (entered, observed) = oneshot::channel();
        let (resume, waiting) = oneshot::channel();
        assert!(
            self.inner
                .idle_owner_close_probe
                .lock()
                .unwrap()
                .replace(IdleOwnerClosePause {
                    entered,
                    resume: waiting
                })
                .is_none()
        );
        (observed, resume)
    }
}
