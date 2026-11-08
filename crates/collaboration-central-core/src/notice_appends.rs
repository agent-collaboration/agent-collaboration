//! Which machines got a notice appended during the running event.
//!
//! The kernel runs each event inside a scope that is set around every poll of
//! the event's future; a notice append names its machine to that scope, and the
//! event's post-event push reads only those machines. An object call served
//! inline runs its own event inside its own scope, so its appends are its own.
//! A rolled-back append still names its machine, which costs that push one read.
//! An append outside any event (a test driving the runtime directly) names no
//! machine; its notice is pushed on the machine's next catch-up.

use fleet_protocol::PeerId;
use std::{
    cell::RefCell,
    collections::BTreeSet,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
    task::{Context, Poll},
};

type Appended = Arc<Mutex<BTreeSet<PeerId>>>;

thread_local! {
    static CURRENT: RefCell<Option<Appended>> = const { RefCell::new(None) };
}

/// Run `inner` as one event: appends made while it is polled are its own.
pub(crate) fn scoped<F: Future>(inner: F) -> Scoped<F> {
    Scoped {
        appended: Appended::default(),
        inner: Box::pin(inner),
    }
}

/// Name the machine whose stream just got a notice to the running event.
pub(crate) fn noted(peer: PeerId) {
    CURRENT.with(|current| {
        if let Some(appended) = current.borrow().as_ref() {
            appended
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(peer);
        }
    });
}

/// Take the machines the running event has appended to so far.
pub(crate) fn take() -> BTreeSet<PeerId> {
    CURRENT.with(|current| {
        current
            .borrow()
            .as_ref()
            .map(|appended| {
                std::mem::take(&mut *appended.lock().unwrap_or_else(PoisonError::into_inner))
            })
            .unwrap_or_default()
    })
}

pub(crate) struct Scoped<F> {
    appended: Appended,
    inner: Pin<Box<F>>,
}

impl<F: Future> Future for Scoped<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let previous = CURRENT.with(|current| current.replace(Some(this.appended.clone())));
        let _restore = RestoreOuter(previous);
        this.inner.as_mut().poll(context)
    }
}

struct RestoreOuter(Option<Appended>);

impl Drop for RestoreOuter {
    fn drop(&mut self) {
        let outer = self.0.take();
        CURRENT.with(|current| *current.borrow_mut() = outer);
    }
}

#[cfg(test)]
#[path = "notice_appends_test.rs"]
mod tests;
