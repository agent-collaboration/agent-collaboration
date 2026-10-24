//! Committed changes wake readers; only the receiver's stored status proves progress.

use durable_steps::{CallFailure, Uuid};
use futures_channel::oneshot;
use runtime_primitives::SyncTime;
use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::{Arc, Mutex, Weak},
};

struct PendingChange {
    deadline: SyncTime,
    changed: oneshot::Sender<()>,
}

#[derive(Default)]
pub(crate) struct CopyChangeNotifier {
    pending: Mutex<BTreeMap<Uuid, PendingChange>>,
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
    pub(crate) fn subscribe(
        self: &Arc<Self>,
        identity: Uuid,
        deadline: SyncTime,
    ) -> Result<CopyChangeWaiter, CallFailure> {
        let (changed, received) = oneshot::channel();
        let mut pending = self.pending.lock().map_err(|_| CallFailure::AnswerLost)?;
        match pending.entry(identity) {
            Entry::Vacant(entry) => {
                entry.insert(PendingChange { deadline, changed });
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
            for (_, pending) in removed {
                let _ = pending.changed.send(());
            }
        }
    }

    pub(crate) fn expire(&self, now: SyncTime) {
        let removed = self.pending.lock().ok().map(|mut pending| {
            let mut expired = Vec::new();
            for (identity, waiting) in pending.iter() {
                if waiting.deadline <= now {
                    expired.push(*identity);
                }
            }
            expired
                .into_iter()
                .filter_map(|identity| pending.remove(&identity))
                .collect::<Vec<_>>()
        });
        // Expiry drops the sender, producing AnswerLost rather than progress.
        drop(removed);
    }

    pub(crate) fn next_deadline(&self) -> Option<SyncTime> {
        self.pending
            .lock()
            .ok()
            .and_then(|pending| pending.values().map(|waiting| waiting.deadline).min())
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
#[allow(
    clippy::panic_in_result_fn,
    reason = "Assertions expose broken notification guarantees."
)]
mod tests {
    use super::*;

    #[test]
    fn a_commit_between_subscribe_and_wait_remains_ready_for_every_reader()
    -> Result<(), Box<dyn std::error::Error>> {
        let changes = Arc::new(CopyChangeNotifier::default());
        let deadline = SyncTime::from_unix_millis(100);
        let mut first = changes
            .subscribe(Uuid::now_v7(), deadline)
            .map_err(|error| format!("{error:?}"))?;
        let mut second = changes
            .subscribe(Uuid::now_v7(), deadline)
            .map_err(|error| format!("{error:?}"))?;

        changes.signal_committed();

        assert_eq!(first.changed.try_recv()?, Some(()));
        assert_eq!(second.changed.try_recv()?, Some(()));
        assert_eq!(changes.next_deadline(), None);
        Ok(())
    }

    #[test]
    fn expiry_and_instance_loss_cancel_the_wait_without_reporting_a_change()
    -> Result<(), Box<dyn std::error::Error>> {
        let changes = Arc::new(CopyChangeNotifier::default());
        let mut waiter = changes
            .subscribe(Uuid::now_v7(), SyncTime::from_unix_millis(100))
            .map_err(|error| format!("{error:?}"))?;

        changes.expire(SyncTime::from_unix_millis(99));
        assert_eq!(waiter.changed.try_recv()?, None);
        changes.expire(SyncTime::from_unix_millis(100));
        assert!(waiter.changed.try_recv().is_err());
        let mut replacement = changes
            .subscribe(Uuid::now_v7(), SyncTime::from_unix_millis(200))
            .map_err(|error| format!("{error:?}"))?;
        drop(changes);

        assert!(replacement.changed.try_recv().is_err());
        Ok(())
    }
}
