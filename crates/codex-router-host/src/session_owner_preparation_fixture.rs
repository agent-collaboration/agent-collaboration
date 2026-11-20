//! Deterministic controls at the Host preparation and create adoption ports.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::oneshot;

#[derive(Default)]
pub(crate) struct PreparationFixture {
    restore_pause: Mutex<Option<PreparationPause>>,
    create_pause: Mutex<Option<PreparationPause>>,
    restore_fault: AtomicBool,
}

struct PreparationPause {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

impl PreparationFixture {
    pub(crate) fn pause_restore(&self) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        Self::arm(&self.restore_pause)
    }

    pub(crate) fn pause_create(&self) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        Self::arm(&self.create_pause)
    }

    pub(crate) fn fault_restore(&self) {
        self.restore_fault.store(true, Ordering::Release);
    }

    pub(crate) fn fault_pending(&self) -> bool {
        self.restore_fault.load(Ordering::Acquire)
    }

    pub(crate) fn take_restore_fault(&self) -> bool {
        self.restore_fault.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn hold_restore(&self) -> Option<oneshot::Receiver<()>> {
        Self::enter(&self.restore_pause)
    }

    pub(crate) fn hold_create(&self) -> Option<oneshot::Receiver<()>> {
        Self::enter(&self.create_pause)
    }

    fn arm(slot: &Mutex<Option<PreparationPause>>) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (entered, reaching) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let previous = slot
            .lock()
            .expect("fixture lock")
            .replace(PreparationPause {
                entered,
                release: released,
            });
        assert!(previous.is_none(), "the prior pause was never reached");
        (reaching, release)
    }

    fn enter(slot: &Mutex<Option<PreparationPause>>) -> Option<oneshot::Receiver<()>> {
        let pause = slot.lock().expect("fixture lock").take()?;
        let _test_withdrew_pause = pause.entered.send(());
        Some(pause.release)
    }
}
