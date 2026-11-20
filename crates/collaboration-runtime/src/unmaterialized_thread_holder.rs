//! Host-lifetime ownership of retained Codex bindings and detached prompt drains.
use codex_acp_adapter::{AcpSessionBinding, HeldBindingCheckout, UnmaterializedBindingStore};
use std::{collections::BTreeMap, sync::Mutex};

enum HeldThread {
    Ready(Box<AcpSessionBinding>),
    Busy,
}

#[derive(Default)]
pub struct UnmaterializedThreadHolder {
    threads: Mutex<BTreeMap<String, HeldThread>>,
    host_tasks: tokio_util::task::TaskTracker,
}

impl UnmaterializedThreadHolder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn contains(&self, session_id: &str) -> bool {
        self.threads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(session_id)
    }

    pub async fn drain_host_tasks(&self) {
        self.host_tasks.close();
        self.host_tasks.wait().await;
    }

    pub(crate) fn checkout_for_delivery(&self, session_id: &str) -> HeldBindingCheckout {
        // Delivery has a loaded-thread path. Only unmaterialized creates
        // need its special held-connection path and idle assumption.
        self.checkout_matching(session_id, AcpSessionBinding::is_unmaterialized)
    }

    fn checkout_matching(
        &self,
        session_id: &str,
        matches: impl FnOnce(&AcpSessionBinding) -> bool,
    ) -> HeldBindingCheckout {
        let mut threads = self
            .threads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match threads.get_mut(session_id) {
            Some(slot @ HeldThread::Ready(_)) => {
                if matches!(slot, HeldThread::Ready(binding) if matches(binding))
                    && let HeldThread::Ready(binding) = std::mem::replace(slot, HeldThread::Busy)
                {
                    HeldBindingCheckout::Ready(binding)
                } else {
                    HeldBindingCheckout::Missing
                }
            }
            Some(HeldThread::Busy) => HeldBindingCheckout::Busy,
            None => HeldBindingCheckout::Missing,
        }
    }
}

/// Clears a checked-out slot if dispatch exits before it can restore or finish it.
pub(crate) struct HeldBindingCleanup<'a> {
    holder: &'a UnmaterializedThreadHolder,
    session_id: String,
    active: bool,
}

impl<'a> HeldBindingCleanup<'a> {
    pub(crate) fn new(holder: &'a UnmaterializedThreadHolder, session_id: &str) -> Self {
        Self {
            holder,
            session_id: session_id.into(),
            active: true,
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.active = false;
    }

    pub(crate) fn finish(&mut self) {
        self.holder.finish(&self.session_id);
        self.disarm();
    }
}

impl Drop for HeldBindingCleanup<'_> {
    fn drop(&mut self) {
        if self.active {
            self.holder.finish(&self.session_id);
        }
    }
}

impl UnmaterializedBindingStore for UnmaterializedThreadHolder {
    fn hold(&self, binding: AcpSessionBinding) {
        if !binding.has_scratch_lease() {
            return;
        }
        self.threads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                binding.session_id().to_owned(),
                HeldThread::Ready(Box::new(binding)),
            );
    }

    fn checkout(&self, session_id: &str) -> HeldBindingCheckout {
        self.checkout_matching(session_id, |_| true)
    }

    fn restore(&self, binding: AcpSessionBinding) {
        self.hold(binding);
    }

    fn finish(&self, session_id: &str) {
        let mut threads = self
            .threads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(threads.get(session_id), Some(HeldThread::Busy)) {
            threads.remove(session_id);
        }
    }

    fn host_tasks(&self) -> tokio_util::task::TaskTracker {
        self.host_tasks.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_drains_in_flight_codex_create_task() {
        let holder = std::sync::Arc::new(UnmaterializedThreadHolder::new());
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        holder.host_tasks().spawn(async move {
            let _entered = entered_tx.send(());
            let _released = release_rx.await;
        });
        entered_rx.await.expect("create task started");
        let draining = tokio::spawn({
            let holder = std::sync::Arc::clone(&holder);
            async move { holder.drain_host_tasks().await }
        });
        tokio::task::yield_now().await;
        assert!(!draining.is_finished());
        let _released = release_tx.send(());
        draining.await.expect("create tasks drained");
    }
}
