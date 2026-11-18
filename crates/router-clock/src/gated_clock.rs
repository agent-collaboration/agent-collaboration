//! A manual clock whose moves and engine drives exclude each other.
//!
//! A drive takes the shared side before it samples its batch time and holds it
//! until its final publication; every move takes the exclusive side. A move
//! therefore waits for every held drive, and the lock's fairness keeps a new
//! drive from starting while a move waits or runs. Test support only.
use crate::{ClockAdvanceError, ClockInstant, ManualClock, RuntimeClock};
use chrono::{DateTime, Utc};
use runtime_primitives::{Clock, SyncTime};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::{OwnedRwLockReadGuard, RwLock, RwLockWriteGuard, watch};

/// Held by a drive from before it samples its time until its final publication.
pub struct DriveGuard {
    _shared: OwnedRwLockReadGuard<()>,
}

pub struct GatedClock {
    clock: ManualClock,
    gate: Arc<RwLock<()>>,
    waiting_moves: watch::Sender<usize>,
    waiting_drives: watch::Sender<usize>,
}

/// The exclusive side, held across a caller's step: no drive runs meanwhile.
pub struct HeldMove<'clock> {
    clock: &'clock GatedClock,
    _exclusive: RwLockWriteGuard<'clock, ()>,
}

impl HeldMove<'_> {
    pub fn advance(&self, step: Duration) -> Result<(), ClockAdvanceError> {
        self.clock.clock.advance(step)
    }
    pub fn advance_to(&self, deadline: SyncTime) -> Result<(), ClockAdvanceError> {
        self.clock.clock.advance_to(deadline)
    }
    pub fn set_utc(&self, utc: DateTime<Utc>) {
        self.clock.clock.set_utc(utc);
    }
}

struct Waiter(watch::Sender<usize>);
impl Waiter {
    fn enter(count: &watch::Sender<usize>) -> Self {
        count.send_modify(|count| *count += 1);
        Self(count.clone())
    }
}
impl Drop for Waiter {
    fn drop(&mut self) {
        self.0.send_modify(|count| *count -= 1);
    }
}

/// Awaits `acquire`, counting the caller as waiting while it is pending.
async fn counted<T>(count: &watch::Sender<usize>, acquire: impl Future<Output = T>) -> T {
    let mut acquire = std::pin::pin!(acquire);
    let mut waiter = None;
    std::future::poll_fn(|context| match acquire.as_mut().poll(context) {
        std::task::Poll::Pending => {
            waiter.get_or_insert_with(|| Waiter::enter(count));
            std::task::Poll::Pending
        }
        ready @ std::task::Poll::Ready(_) => ready,
    })
    .await
}

impl GatedClock {
    #[must_use]
    pub fn new(clock: ManualClock) -> Self {
        Self {
            clock,
            gate: Arc::new(RwLock::new(())),
            waiting_moves: watch::channel(0).0,
            waiting_drives: watch::channel(0).0,
        }
    }

    pub fn starting_at(utc: SyncTime) -> Result<Self, ClockAdvanceError> {
        ManualClock::starting_at(utc).map(Self::new)
    }

    /// Takes the exclusive side once every held drive has finished.
    pub async fn hold_move(&self) -> HeldMove<'_> {
        HeldMove {
            clock: self,
            _exclusive: counted(&self.waiting_moves, self.gate.write()).await,
        }
    }

    pub async fn advance(&self, step: Duration) -> Result<(), ClockAdvanceError> {
        self.hold_move().await.advance(step)
    }

    pub async fn advance_to(&self, deadline: SyncTime) -> Result<(), ClockAdvanceError> {
        self.hold_move().await.advance_to(deadline)
    }

    pub async fn set_utc(&self, utc: DateTime<Utc>) {
        self.hold_move().await.set_utc(utc);
    }

    /// The one deliberate overlap: sets the wall clock while an engine batch is
    /// held on purpose, without waiting for held drives.
    pub fn set_utc_while_a_batch_is_held(&self, utc: DateTime<Utc>) {
        self.clock.set_utc(utc);
    }

    /// The shared side, for one drive.
    pub async fn hold_drive(&self) -> DriveGuard {
        DriveGuard {
            _shared: counted(&self.waiting_drives, self.gate.clone().read_owned()).await,
        }
    }

    /// Resolves once a move is waiting for held drives.
    pub async fn until_move_waiting(&self) {
        let mut waiting = self.waiting_moves.subscribe();
        let _ = waiting.wait_for(|waiting| *waiting > 0).await;
    }

    /// Resolves once a drive is waiting behind a move.
    pub async fn until_drive_waiting(&self) {
        let mut waiting = self.waiting_drives.subscribe();
        let _ = waiting.wait_for(|waiting| *waiting > 0).await;
    }

    /// A causal observation that `deadline` was checked at the current time.
    pub async fn not_due_check_now(&self, deadline: ClockInstant) {
        self.clock.not_due_check_now(deadline).await;
    }
}

impl Clock for GatedClock {
    fn now(&self) -> SyncTime {
        self.clock.now()
    }
}

impl RuntimeClock for GatedClock {
    fn utc_now(&self) -> DateTime<Utc> {
        self.clock.utc_now()
    }
    fn monotonic_now(&self) -> ClockInstant {
        self.clock.monotonic_now()
    }
    fn sleep_until(&self, deadline: ClockInstant) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.clock.sleep_until(deadline)
    }
    fn has_not_due_check_now(&self, deadline: ClockInstant) -> bool {
        self.clock.has_not_due_check_now(deadline)
    }
    fn wall_deadline(&self, deadline: SyncTime) -> ClockInstant {
        self.clock.wall_deadline(deadline)
    }
    fn drive_guard(&self) -> Pin<Box<dyn Future<Output = Option<DriveGuard>> + Send + '_>> {
        Box::pin(async { Some(self.hold_drive().await) })
    }
}

#[cfg(test)]
#[path = "gated_clock_test.rs"]
mod tests;
