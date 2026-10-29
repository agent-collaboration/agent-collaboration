//! Lifecycle checkpoints wake conformance readers; stored rows remain the oracle.
use durable_steps::{CallFailure, Uuid};
use futures_channel::oneshot;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};

#[derive(Default)]
pub(super) struct RevocationProgress {
    waiting: Mutex<BTreeMap<Uuid, oneshot::Sender<()>>>,
}

pub(super) struct ProgressWaiter {
    owner: Weak<RevocationProgress>,
    identity: Uuid,
    changed: oneshot::Receiver<()>,
}

pub(crate) struct ProgressNotification(Arc<RevocationProgress>);

impl RevocationProgress {
    /// Register before reading the owning row, retaining a change during that read.
    pub(super) fn subscribe(
        self: &Arc<Self>,
        identity: Uuid,
    ) -> Result<ProgressWaiter, CallFailure> {
        let (changed, received) = oneshot::channel();
        let mut waiting = self.waiting.lock().map_err(|_| CallFailure::AnswerLost)?;
        if waiting.contains_key(&identity) {
            return Err(CallFailure::AnswerLost);
        }
        waiting.insert(identity, changed);
        Ok(ProgressWaiter {
            owner: Arc::downgrade(self),
            identity,
            changed: received,
        })
    }

    pub(super) fn notify(&self) {
        let waiting = self
            .waiting
            .lock()
            .ok()
            .map(|mut waiting| std::mem::take(&mut *waiting));
        // Resuming a reader can re-enter the object. Wake after releasing the lock.
        if let Some(waiting) = waiting {
            for (_, changed) in waiting {
                let _received = changed.send(());
            }
        }
    }

    pub(super) fn notify_when_finished(self: &Arc<Self>) -> ProgressNotification {
        ProgressNotification(self.clone())
    }
}

impl ProgressWaiter {
    pub(super) async fn changed(mut self) -> Result<(), CallFailure> {
        (&mut self.changed)
            .await
            .map_err(|_| CallFailure::AnswerLost)
    }
}

impl Drop for ProgressWaiter {
    fn drop(&mut self) {
        let removed = self.owner.upgrade().and_then(|owner| {
            owner
                .waiting
                .lock()
                .ok()
                .and_then(|mut waiting| waiting.remove(&self.identity))
        });
        drop(removed);
    }
}

impl Drop for ProgressNotification {
    fn drop(&mut self) {
        self.0.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checkpoint_between_subscribe_and_wait_is_retained_for_each_reader() {
        let progress = Arc::new(RevocationProgress::default());
        let mut first = progress.subscribe(Uuid::now_v7()).expect("first observer");
        let mut second = progress.subscribe(Uuid::now_v7()).expect("second observer");
        progress.notify();
        assert_eq!(
            first.changed.try_recv().expect("first checkpoint"),
            Some(())
        );
        assert_eq!(
            second.changed.try_recv().expect("second checkpoint"),
            Some(())
        );
    }

    #[test]
    fn cancelling_an_observer_releases_its_subscription_without_a_checkpoint() {
        let progress = Arc::new(RevocationProgress::default());
        let waiting = progress.subscribe(Uuid::now_v7()).expect("observer");
        assert_eq!(progress.waiting.lock().expect("observers").len(), 1);
        drop(waiting);
        assert!(progress.waiting.lock().expect("observers").is_empty());
    }

    #[test]
    fn losing_the_owner_does_not_report_progress() {
        let progress = Arc::new(RevocationProgress::default());
        let mut waiting = progress.subscribe(Uuid::now_v7()).expect("observer");
        drop(progress);
        assert!(waiting.changed.try_recv().is_err());
    }
}
