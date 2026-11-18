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
    turns: Mutex<HashMap<SessionRef, Arc<tokio::sync::Mutex<()>>>>,
    /// Told when the next change has to wait for its session's turn.
    #[cfg(any(test, feature = "test-support"))]
    wait_notice: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

/// One change's turn on its session, released when dropped.
pub(super) struct SessionModeTurn<'order> {
    order: &'order SessionModeOrder,
    target: SessionRef,
    held: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl SessionModeOrder {
    /// Waits for `target`'s turn: changes enter in the order they asked.
    pub(super) async fn enter(&self, target: &SessionRef) -> SessionModeTurn<'_> {
        let turn = Arc::clone(
            self.turns
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(target.clone())
                .or_default(),
        );
        let held = match Arc::clone(&turn).try_lock_owned() {
            Ok(held) => held,
            Err(_) => {
                #[cfg(any(test, feature = "test-support"))]
                self.notice_wait();
                turn.lock_owned().await
            }
        };
        SessionModeTurn {
            order: self,
            target: target.clone(),
            held: Some(held),
        }
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
        // Only the map still holds it: no change holds or waits on this turn.
        if turns
            .get(&self.target)
            .is_some_and(|turn| Arc::strong_count(turn) == 1)
        {
            turns.remove(&self.target);
        }
    }
}

#[cfg(test)]
#[path = "explicit_mode_order_test.rs"]
mod tests;
