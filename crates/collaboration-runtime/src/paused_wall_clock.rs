//! The wall clock tests inject beside Tokio's paused clock. It reads Tokio's paused time
//! plus a fixed base, so every advance (by `tokio::time::advance` or by the paused
//! runtime jumping to its next timer) moves both clocks together.
use runtime_primitives::{Clock, SyncTime};

pub struct PausedWallClock {
    base: SyncTime,
    started: tokio::time::Instant,
}

impl PausedWallClock {
    /// Starts at `base`. Create it inside a test runtime started paused.
    #[must_use]
    pub fn starting_at(base: SyncTime) -> Self {
        Self {
            base,
            started: tokio::time::Instant::now(),
        }
    }
}

impl Clock for PausedWallClock {
    fn now(&self) -> SyncTime {
        self.base.after(self.started.elapsed())
    }
}
