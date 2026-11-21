//! Hold an accepted store write or observe a waiter at its actual owner boundary.

use collaboration_protocol::{OperationId, SessionRef};
use std::{collections::BTreeMap, sync::Mutex};
use tokio::sync::oneshot;

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum BindingFixtureWait {
    ExplicitSetting,
    LatePromptRepair,
}

#[derive(Default)]
pub(crate) struct BindingStoreFixture {
    terminal_write: WritePause<OperationId>,
    mode_write: WritePause<(SessionRef, String)>,
    waits: Mutex<BTreeMap<(SessionRef, BindingFixtureWait), oneshot::Sender<()>>>,
}

struct WritePause<Key>(Mutex<Option<(Key, Pause)>>);

impl<Key> Default for WritePause<Key> {
    fn default() -> Self {
        Self(Mutex::default())
    }
}

struct Pause {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

impl<Key> WritePause<Key> {
    fn arm(&self, key: Key) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (entered, reaching) = oneshot::channel();
        let (releasing, release) = oneshot::channel();
        let prior = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .replace((key, Pause { entered, release }));
        assert!(prior.is_none(), "the prior write pause was never reached");
        (reaching, releasing)
    }

    async fn hold(&self, matches: impl FnOnce(&Key) -> bool) {
        let pause = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take_if(|(key, _)| matches(key));
        if let Some((_, pause)) = pause {
            let _entered = pause.entered.send(());
            let _released = pause.release.await;
        }
    }
}

impl BindingStoreFixture {
    pub(crate) fn pause_terminal_write(
        &self,
        operation: OperationId,
    ) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        self.terminal_write.arm(operation)
    }

    pub(crate) async fn hold_terminal_write(&self, operation: &OperationId) {
        self.terminal_write
            .hold(|waiting| waiting == operation)
            .await;
    }

    pub(crate) fn pause_mode_write(
        &self,
        target: SessionRef,
        value: String,
    ) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        self.mode_write.arm((target, value))
    }

    pub(crate) async fn hold_mode_write(&self, target: &SessionRef, value: &str) {
        self.mode_write
            .hold(|(waiting, requested)| waiting == target && requested == value)
            .await;
    }

    pub(crate) fn notice_wait(
        &self,
        target: SessionRef,
        wait: BindingFixtureWait,
    ) -> oneshot::Receiver<()> {
        let (send, receive) = oneshot::channel();
        let prior = self
            .waits
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert((target, wait), send);
        assert!(prior.is_none(), "the prior fixture wait was never reached");
        receive
    }

    pub(crate) fn waiting(&self, target: &SessionRef, wait: BindingFixtureWait) {
        if let Some(waiter) = self
            .waits
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&(target.clone(), wait))
        {
            let _receiver_gone = waiter.send(());
        }
    }
}
