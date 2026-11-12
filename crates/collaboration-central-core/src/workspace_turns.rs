//! One write turn per workspace (Program Design, "Two requests at once").
//!
//! Central waits on a workspace store between reading and writing, so a request that
//! decides something takes its workspace's turn, reads, writes its batch and releases.
//! The turn is an in-memory async lock: one live central object holds every turn.

use message_board::WorkspaceId;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::{OwnedMutexGuard, watch};

#[derive(Default)]
pub struct WorkspaceTurns {
    turns: Mutex<BTreeMap<WorkspaceId, Arc<TurnSlot>>>,
}

struct TurnSlot {
    lock: Arc<tokio::sync::Mutex<()>>,
    /// Requests waiting to take this turn, not counting its holder.
    waiting: watch::Sender<usize>,
}

/// Holding this value is holding the workspace's turn; dropping it releases the turn.
pub struct WorkspaceTurn {
    _held: OwnedMutexGuard<()>,
}

/// Counts one waiter for as long as it exists, so a cancelled wait still uncounts itself.
struct WaitingMark {
    slot: Arc<TurnSlot>,
}

impl WaitingMark {
    fn enter(slot: Arc<TurnSlot>) -> Self {
        slot.waiting
            .send_modify(|waiting| *waiting = waiting.saturating_add(1));
        Self { slot }
    }
}

impl Drop for WaitingMark {
    fn drop(&mut self) {
        self.slot
            .waiting
            .send_modify(|waiting| *waiting = waiting.saturating_sub(1));
    }
}

impl WorkspaceTurns {
    fn slot(&self, workspace: &WorkspaceId) -> Arc<TurnSlot> {
        let mut turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        turns
            .entry(workspace.clone())
            .or_insert_with(|| {
                Arc::new(TurnSlot {
                    lock: Arc::new(tokio::sync::Mutex::new(())),
                    waiting: watch::Sender::new(0),
                })
            })
            .clone()
    }

    /// Waits for the workspace's turn and holds it until the returned value drops.
    pub async fn take(&self, workspace: &WorkspaceId) -> WorkspaceTurn {
        let slot = self.slot(workspace);
        let lock = slot.lock.clone();
        let waiting = WaitingMark::enter(slot);
        let held = lock.lock_owned().await;
        drop(waiting);
        WorkspaceTurn { _held: held }
    }

    /// How many requests wait for this workspace's turn (test seam).
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn waiting(&self, workspace: &WorkspaceId) -> usize {
        *self.slot(workspace).waiting.borrow()
    }

    /// Resolves once at least `count` requests wait for this workspace's turn (test seam).
    #[cfg(feature = "test-support")]
    pub async fn until_waiting(&self, workspace: &WorkspaceId, count: usize) {
        let mut waiting = self.slot(workspace).waiting.subscribe();
        // The sender lives in the slot, which the map keeps; the wait cannot be orphaned.
        let _ = waiting.wait_for(|waiting| *waiting >= count).await;
    }
}

#[cfg(test)]
#[path = "workspace_turns_test.rs"]
mod tests;
