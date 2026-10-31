//! Test observations of completed machine-session operations and reactions.
use tokio::sync::watch;

#[derive(Clone)]
pub(super) struct MachineProgress {
    completed: watch::Sender<u64>,
}

impl Default for MachineProgress {
    fn default() -> Self {
        Self {
            completed: watch::channel(0).0,
        }
    }
}

impl MachineProgress {
    pub(super) fn record_completion(&self) {
        self.completed
            .send_modify(|counter| *counter = counter.wrapping_add(1));
    }

    pub(super) fn observe(&self) -> watch::Receiver<u64> {
        self.completed.subscribe()
    }
}
