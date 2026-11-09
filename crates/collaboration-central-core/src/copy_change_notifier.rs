//! Committed changes wake readers; only the receiver's stored status proves progress.

use durable_steps::{CallFailure, Uuid};
use futures_channel::oneshot;
use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::{Arc, Mutex, Weak},
};

#[derive(Default)]
pub(crate) struct CopyChangeNotifier {
    pending: Mutex<BTreeMap<Uuid, oneshot::Sender<()>>>,
    #[cfg(feature = "conformance")]
    notifications: std::sync::atomic::AtomicU64,
}

pub(crate) struct CopyChangeWaiter {
    notifier: Weak<CopyChangeNotifier>,
    identity: Uuid,
    changed: oneshot::Receiver<()>,
}

impl CopyChangeNotifier {
    /// Register before reading status, so a commit in the read/wait gap is retained.
    /// The caller bounds the wait with its event's deadline.
    pub(crate) fn subscribe(
        self: &Arc<Self>,
        identity: Uuid,
    ) -> Result<CopyChangeWaiter, CallFailure> {
        let (changed, received) = oneshot::channel();
        let mut pending = self.pending.lock().map_err(|_| CallFailure::AnswerLost)?;
        match pending.entry(identity) {
            Entry::Vacant(entry) => {
                entry.insert(changed);
            }
            Entry::Occupied(_) => return Err(CallFailure::AnswerLost),
        }
        Ok(CopyChangeWaiter {
            notifier: Arc::downgrade(self),
            identity,
            changed: received,
        })
    }

    pub(crate) fn signal_committed(&self) {
        #[cfg(feature = "conformance")]
        self.notifications
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let removed = self
            .pending
            .lock()
            .ok()
            .map(|mut pending| std::mem::take(&mut *pending));
        // Waking readers can re-enter the object. Never wake under this lock.
        if let Some(removed) = removed {
            for (_, changed) in removed {
                let _ = changed.send(());
            }
        }
    }

    #[cfg(feature = "conformance")]
    pub(crate) fn waiting(&self) -> Result<usize, durable_steps::StoreError> {
        Ok(self
            .pending
            .lock()
            .map_err(durable_steps::StoreError::backend)?
            .len())
    }

    #[cfg(feature = "conformance")]
    pub(crate) fn notifications(&self) -> u64 {
        self.notifications.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl CopyChangeWaiter {
    pub(crate) async fn changed(mut self) -> Result<(), CallFailure> {
        (&mut self.changed)
            .await
            .map_err(|_| CallFailure::AnswerLost)
    }
}

impl Drop for CopyChangeWaiter {
    fn drop(&mut self) {
        let removed = self.notifier.upgrade().and_then(|notifier| {
            notifier
                .pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&self.identity))
        });
        drop(removed);
    }
}

#[cfg(test)]
#[path = "copy_change_notifier_test.rs"]
mod tests;
