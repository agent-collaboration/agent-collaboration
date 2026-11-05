//! Causal notification for a held invocation or observed reply, without a timer.
use crate::RuntimeControlFailure;
use std::{
    future::poll_fn,
    sync::Mutex,
    task::{Poll, Waker},
};

#[derive(Default)]
struct SignalState {
    signalled: bool,
    waiters: Vec<Waker>,
}
#[derive(Default)]
pub(super) struct Signal(Mutex<SignalState>);
impl Signal {
    pub(super) fn signal(&self) -> Result<(), RuntimeControlFailure> {
        let waiters = {
            let mut state = self.0.lock().map_err(|_| RuntimeControlFailure::Poisoned)?;
            state.signalled = true;
            std::mem::take(&mut state.waiters)
        };
        for waiter in waiters {
            waiter.wake();
        }
        Ok(())
    }
    pub(super) async fn wait(&self) -> Result<(), RuntimeControlFailure> {
        poll_fn(|context| {
            let Ok(mut state) = self.0.lock() else {
                return Poll::Ready(Err(RuntimeControlFailure::Poisoned));
            };
            if state.signalled {
                return Poll::Ready(Ok(()));
            }
            if !state
                .waiters
                .iter()
                .any(|waiter| waiter.will_wake(context.waker()))
            {
                state.waiters.push(context.waker().clone());
            }
            Poll::Pending
        })
        .await
    }
}
