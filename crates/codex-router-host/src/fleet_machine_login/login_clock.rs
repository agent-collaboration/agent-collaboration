use std::{future::Future, pin::Pin};

use runtime_primitives::{Clock, SyncTime, SystemClock, TokioWakeTimer, WakeScheduler};

pub(super) type LoginFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The Host supplies time and waits together; tests supply neither from Tokio time.
pub trait FleetLoginClock: Send + Sync {
    fn now(&self) -> SyncTime;
    fn wait_until(&self, deadline: SyncTime) -> LoginFuture<'_, Result<(), ()>>;
}

pub(super) struct HostFleetLoginClock;
impl FleetLoginClock for HostFleetLoginClock {
    fn now(&self) -> SyncTime {
        SystemClock.now()
    }
    fn wait_until(&self, deadline: SyncTime) -> LoginFuture<'_, Result<(), ()>> {
        Box::pin(async move {
            // Each waiting operation owns its timer, so login polling and token
            // renewal cannot consume one another's wakes.
            let timer = TokioWakeTimer::new(SystemClock);
            timer.wake_at(deadline).await.map_err(|_| ())?;
            timer.wait().await;
            Ok(())
        })
    }
}
