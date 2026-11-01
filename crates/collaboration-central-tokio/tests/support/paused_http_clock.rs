#![cfg(test)]
//! Reads only the paused Tokio clock for per-case before/after assertions.
//! Call exclusively inside `#[tokio::test(start_paused = true)]` runtimes.
//! This never caps a wait, advances time, or reads the standard clock.

pub struct PausedHttpClock {
    captured_at: tokio::time::Instant,
}

#[derive(Debug, thiserror::Error)]
pub enum UnexpectedClockAdvance {
    #[error(
        "{case_name}: paused HTTP clock advanced by {advanced_by:?} without an explicit instruction"
    )]
    Advanced {
        case_name: String,
        advanced_by: std::time::Duration,
    },
    #[error("{case_name}: paused HTTP clock moved backwards without an explicit instruction")]
    MovedBackwards { case_name: String },
}

impl PausedHttpClock {
    pub fn before_case() -> Self {
        Self {
            captured_at: tokio::time::Instant::now(),
        }
    }

    pub fn require_no_advance(self, case_name: &str) -> Result<(), UnexpectedClockAdvance> {
        let after = tokio::time::Instant::now();
        if after == self.captured_at {
            return Ok(());
        }
        Err(match after.checked_duration_since(self.captured_at) {
            Some(advanced_by) => UnexpectedClockAdvance::Advanced {
                case_name: case_name.to_owned(),
                advanced_by,
            },
            None => UnexpectedClockAdvance::MovedBackwards {
                case_name: case_name.to_owned(),
            },
        })
    }
}

/// Keeps the paused clock stationary while real async HTTP is in flight.
/// Tokio documents that a running blocking task inhibits auto-advance. This
/// task waits only for an owned channel event; it runs no I/O, clock or timer.
/// Call finish after the requests to release and join it before cleanup.
pub struct PausedHttpIo {
    state: IoClockState,
}
enum IoClockState {
    Active {
        release: std::sync::mpsc::Sender<()>,
        task: tokio::task::JoinHandle<()>,
    },
    Released,
}
#[derive(Debug, thiserror::Error)]
pub enum PausedIoFailure {
    #[error("paused HTTP clock task did not report startup")]
    StartupClosed,
    #[error("paused HTTP clock task did not join")]
    Task(#[from] tokio::task::JoinError),
}
impl PausedHttpIo {
    pub async fn start() -> Result<Self, PausedIoFailure> {
        let (release, released) = std::sync::mpsc::channel();
        let (started, ready) = tokio::sync::oneshot::channel();
        let task = tokio::task::spawn_blocking(move || {
            let _ = started.send(());
            let _ = released.recv();
        });
        let hold = Self {
            state: IoClockState::Active { release, task },
        };
        if ready.await.is_err() {
            hold.finish().await?;
            return Err(PausedIoFailure::StartupClosed);
        }
        Ok(hold)
    }
    pub async fn finish(mut self) -> Result<(), PausedIoFailure> {
        match std::mem::replace(&mut self.state, IoClockState::Released) {
            IoClockState::Active { release, task } => {
                let _ = release.send(());
                task.await?;
            }
            IoClockState::Released => {}
        }
        Ok(())
    }
}
impl Drop for PausedHttpIo {
    fn drop(&mut self) {
        // Exceptional exits still release the event; normal paths join above.
        // The owning test runtime also drains its blocking pool on shutdown.
        if let IoClockState::Active { release, .. } =
            std::mem::replace(&mut self.state, IoClockState::Released)
        {
            let _ = release.send(());
        }
    }
}
