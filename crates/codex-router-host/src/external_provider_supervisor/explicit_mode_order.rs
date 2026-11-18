//! One explicit mode change at a time per session (Lead V6b): a change holds
//! its session's turn from before its command reaches the actor until its
//! durable record is written, so the session's recorded mode always ends at
//! the last change the agent applied. A later change waits; another
//! session's change never does.

use collaboration_protocol::SessionRef;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

/// Each session's turn for explicit mode changes; an entry lives only while
/// a change holds or waits on it.
#[derive(Debug, Default)]
pub(super) struct SessionModeOrder {
    turns: Mutex<HashMap<SessionRef, SessionTurn>>,
    /// Told when the next change has to wait for its session's turn.
    #[cfg(any(test, feature = "test-support"))]
    wait_notice: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

/// One session's turn and how many changes hold or wait on it. Counted, not
/// read from the lock's reference count: a cancelled waiter's copies are
/// dropped in no order this module controls.
#[derive(Debug, Default)]
struct SessionTurn {
    lock: Arc<tokio::sync::Mutex<()>>,
    changes: usize,
}

/// One change's claim on its session's turn, holding it or still waiting;
/// dropping it, either way, ends the claim.
pub(super) struct SessionModeTurn<'order> {
    order: &'order SessionModeOrder,
    target: SessionRef,
    held: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl SessionModeOrder {
    /// Waits for `target`'s turn: changes enter in the order they asked.
    pub(super) async fn enter(&self, target: &SessionRef) -> SessionModeTurn<'_> {
        let lock = {
            let mut turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            let turn = turns.entry(target.clone()).or_default();
            turn.changes += 1;
            Arc::clone(&turn.lock)
        };
        // Claimed from here: a change cancelled while it waits drops this
        // claim and ends it, as a change that held the turn does.
        let mut claim = SessionModeTurn {
            order: self,
            target: target.clone(),
            held: None,
        };
        claim.held = Some(match Arc::clone(&lock).try_lock_owned() {
            Ok(held) => held,
            Err(_) => {
                #[cfg(any(test, feature = "test-support"))]
                self.notice_wait();
                lock.lock_owned().await
            }
        });
        claim
    }

    /// The next change that has to wait for its session's turn fires the
    /// returned receiver as it starts waiting.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn notice_next_wait(&self) -> tokio::sync::oneshot::Receiver<()> {
        let (notice, noticed) = tokio::sync::oneshot::channel();
        *self
            .wait_notice
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(notice);
        noticed
    }

    #[cfg(any(test, feature = "test-support"))]
    fn notice_wait(&self) {
        if let Some(notice) = self
            .wait_notice
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _told = notice.send(());
        }
    }

    #[cfg(test)]
    fn sessions_with_turns(&self) -> usize {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

impl Drop for SessionModeTurn<'_> {
    fn drop(&mut self) {
        drop(self.held.take());
        let mut turns = self
            .order
            .turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(turn) = turns.get_mut(&self.target) {
            turn.changes = turn.changes.saturating_sub(1);
            // No change holds or waits on this turn any more.
            if turn.changes == 0 {
                turns.remove(&self.target);
            }
        }
    }
}

#[cfg(test)]
#[path = "explicit_mode_order_test.rs"]
mod tests;
