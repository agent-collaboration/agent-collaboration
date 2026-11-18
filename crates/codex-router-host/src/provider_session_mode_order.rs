//! One mode operation at a time per session of a binding (Lead V6b; review
//! F3). An explicit mode change holds its session's turn from before its
//! command reaches the actor until its durable record is written, so the
//! recorded mode always ends at the last change the agent applied. A restore
//! holds it from reading the session's record until its mode is back, so it
//! never reads a mode a change is still recording. A later operation waits;
//! another session's never does.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

/// Each session's turn for explicit mode changes; an entry lives only while
/// a change holds or waits on it.
#[derive(Debug, Default)]
pub(crate) struct SessionModeOrder {
    turns: Mutex<HashMap<String, SessionTurn>>,
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

/// One operation's claim on its session's turn, holding it or still waiting;
/// dropping it, either way, ends the claim. It owns its order, so a task can
/// carry it to the end of a load.
pub(crate) struct SessionModeTurn {
    order: Arc<SessionModeOrder>,
    session_id: String,
    held: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl SessionModeOrder {
    /// Waits for the session's turn: operations enter in the order they asked.
    pub(crate) async fn enter(self: &Arc<Self>, session_id: &str) -> SessionModeTurn {
        let lock = {
            let mut turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            let turn = turns.entry(session_id.to_owned()).or_default();
            turn.changes += 1;
            Arc::clone(&turn.lock)
        };
        // Claimed from here: a change cancelled while it waits drops this
        // claim and ends it, as a change that held the turn does.
        let mut claim = SessionModeTurn {
            order: Arc::clone(self),
            session_id: session_id.to_owned(),
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
    pub(crate) fn notice_next_wait(&self) -> tokio::sync::oneshot::Receiver<()> {
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

impl Drop for SessionModeTurn {
    fn drop(&mut self) {
        drop(self.held.take());
        let mut turns = self
            .order
            .turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(turn) = turns.get_mut(&self.session_id) {
            turn.changes = turn.changes.saturating_sub(1);
            // No change holds or waits on this turn any more.
            if turn.changes == 0 {
                turns.remove(&self.session_id);
            }
        }
    }
}

#[cfg(test)]
#[path = "provider_session_mode_order_test.rs"]
mod tests;
