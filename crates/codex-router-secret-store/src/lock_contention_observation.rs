//! A test worker observes the OS's actual contention result before awaiting release.
use std::{cell::RefCell, sync::mpsc};

thread_local! {
    static NEXT_CONTENTION: RefCell<Option<mpsc::Sender<()>>> = const { RefCell::new(None) };
}

#[cfg(all(feature = "test-support", debug_assertions))]
pub(crate) struct ArmedContentionObserver;

#[cfg(all(feature = "test-support", debug_assertions))]
pub(crate) fn arm_contention_observer(sender: mpsc::Sender<()>) -> ArmedContentionObserver {
    observe_next_contention(sender);
    ArmedContentionObserver
}

#[cfg(all(feature = "test-support", debug_assertions))]
impl Drop for ArmedContentionObserver {
    fn drop(&mut self) {
        NEXT_CONTENTION.with(|observer| {
            observer.borrow_mut().take();
        });
    }
}

pub(crate) fn observe_next_contention(sender: mpsc::Sender<()>) {
    NEXT_CONTENTION.with(|observer| {
        assert!(
            observer.borrow_mut().replace(sender).is_none(),
            "a lock observer is already armed"
        );
    });
}

pub(crate) fn report_contention() {
    NEXT_CONTENTION.with(|observer| {
        if let Some(sender) = observer.borrow_mut().take() {
            let _observed = sender.send(());
        }
    });
}
