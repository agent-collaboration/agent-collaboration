//! What the running event owes once its transactions commit.
//!
//! The kernel runs each event inside a scope that is set around every poll of
//! the event's future. Work that owes a send names its machine to that scope:
//! a notice append (the event's post-event push sends it) and a newly owed
//! socket request (the post-event step sends it within the window). The
//! post-event step reads only those machines, so an idle event reads nothing.
//! An object call served inline runs its own event inside its own scope, so
//! what it owes is its own. A store whose transaction callback is polled as a
//! separate task (workerd's storage transaction runs it as its own promise)
//! carries the scope across with [`within`]; see `EventScopeStore`.
//! Work that rolls back still names its machine, which costs one read. Work
//! outside any event (a test driving the runtime directly) names no machine;
//! it is sent on the machine's next catch-up.

use fleet_protocol::PeerId;
use std::{
    cell::RefCell,
    collections::BTreeSet,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
    task::{Context, Poll},
};

/// Machines the event owes a send to, by kind of send.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct EventOwed {
    /// Machines whose notice stream got an append.
    pub notices: BTreeSet<PeerId>,
    /// Machines that were owed a new socket request.
    pub requests: BTreeSet<PeerId>,
}

/// The running event's owed work; cloned to carry the scope elsewhere.
pub(crate) type Shared = Arc<Mutex<EventOwed>>;

thread_local! {
    static CURRENT: RefCell<Option<Shared>> = const { RefCell::new(None) };
}

/// Run `inner` as one event: what it owes while it is polled is its own.
pub(crate) fn scoped<F: Future>(inner: F) -> Scoped<F> {
    within(Some(Shared::default()), inner)
}

/// The running event's scope, to carry into work polled outside its polls.
pub(crate) fn current() -> Option<Shared> {
    CURRENT.with(|current| current.borrow().clone())
}

/// Run `inner` inside a scope captured with [`current`].
pub(crate) fn within<F: Future>(owed: Option<Shared>, inner: F) -> Scoped<F> {
    Scoped {
        owed,
        inner: Box::pin(inner),
    }
}

fn note(record: impl FnOnce(&mut EventOwed)) {
    CURRENT.with(|current| {
        if let Some(owed) = current.borrow().as_ref() {
            record(&mut owed.lock().unwrap_or_else(PoisonError::into_inner));
        }
    });
}

/// Name the machine whose notice stream just got an append.
pub(crate) fn owed_notice(peer: PeerId) {
    note(|owed| {
        owed.notices.insert(peer);
    });
}

/// Name the machine that was just owed a socket request.
pub(crate) fn owed_request(peer: PeerId) {
    note(|owed| {
        owed.requests.insert(peer);
    });
}

/// Take what the running event owes so far.
pub(crate) fn take() -> EventOwed {
    CURRENT.with(|current| {
        current
            .borrow()
            .as_ref()
            .map(|owed| std::mem::take(&mut *owed.lock().unwrap_or_else(PoisonError::into_inner)))
            .unwrap_or_default()
    })
}

pub(crate) struct Scoped<F> {
    owed: Option<Shared>,
    inner: Pin<Box<F>>,
}

impl<F: Future> Future for Scoped<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let previous = CURRENT.with(|current| current.replace(this.owed.clone()));
        let _restore = RestoreOuter(previous);
        this.inner.as_mut().poll(context)
    }
}

struct RestoreOuter(Option<Shared>);

impl Drop for RestoreOuter {
    fn drop(&mut self) {
        let outer = self.0.take();
        CURRENT.with(|current| *current.borrow_mut() = outer);
    }
}

#[cfg(test)]
#[path = "event_owed_test.rs"]
mod tests;
