//! Serializes organization-wide writes and lets engine work try without waiting.

/// Dropping the guard releases the organization's write turn.
pub struct TurnGuard<'a> {
    _held: tokio::sync::MutexGuard<'a, ()>,
}

#[derive(Default)]
pub(super) struct CentralTurn {
    lock: tokio::sync::Mutex<()>,
    #[cfg(feature = "test-support")]
    waiting: tokio::sync::watch::Sender<usize>,
}

impl CentralTurn {
    pub(super) fn try_take(&self) -> Option<TurnGuard<'_>> {
        self.lock
            .try_lock()
            .ok()
            .map(|held| TurnGuard { _held: held })
    }

    pub(super) async fn take(&self) -> TurnGuard<'_> {
        #[cfg(not(feature = "test-support"))]
        let held = self.lock.lock().await;
        #[cfg(feature = "test-support")]
        let held = {
            use std::future::Future;
            let mut acquisition = std::pin::pin!(self.lock.lock());
            let mut waiting = None;
            std::future::poll_fn(|context| match acquisition.as_mut().poll(context) {
                std::task::Poll::Pending => {
                    waiting.get_or_insert_with(|| CentralWaiter::enter(self.waiting.clone()));
                    std::task::Poll::Pending
                }
                ready @ std::task::Poll::Ready(_) => ready,
            })
            .await
        };
        TurnGuard { _held: held }
    }

    #[cfg(feature = "test-support")]
    pub(super) async fn until_waiting(&self, count: usize) {
        let mut waiting = self.waiting.subscribe();
        let _ = waiting.wait_for(|waiting| *waiting >= count).await;
    }
}

#[cfg(feature = "test-support")]
struct CentralWaiter(tokio::sync::watch::Sender<usize>);

#[cfg(feature = "test-support")]
impl CentralWaiter {
    fn enter(waiting: tokio::sync::watch::Sender<usize>) -> Self {
        waiting.send_modify(|count| *count += 1);
        Self(waiting)
    }
}

#[cfg(feature = "test-support")]
impl Drop for CentralWaiter {
    fn drop(&mut self) {
        self.0.send_modify(|count| *count -= 1);
    }
}
