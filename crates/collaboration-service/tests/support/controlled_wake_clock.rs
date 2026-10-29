//! Explicit UTC advances drive wake policy; paused Tokio drives the unchanged worker tick.
use chrono::{DateTime, Utc};
use collaboration_service::SubscriptionClock;
use std::{sync::Mutex, time::Duration};

pub struct ControlledWakeClock {
    now: Mutex<DateTime<Utc>>,
}
impl ControlledWakeClock {
    pub fn new() -> super::TestResult<Self> {
        Ok(Self {
            now: Mutex::new(DateTime::from_timestamp(1_800_000_000, 0).ok_or("fixture timestamp")?),
        })
    }

    pub async fn advance(&self, duration: Duration) -> super::TestResult<()> {
        *self.now.lock().map_err(|_| "fixture clock poisoned")? +=
            chrono::Duration::from_std(duration)?;
        tokio::time::advance(duration).await;
        Ok(())
    }
}
impl SubscriptionClock for ControlledWakeClock {
    fn now(&self) -> DateTime<Utc> {
        match self.now.lock() {
            Ok(now) => *now,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }
    fn monotonic_now(&self) -> tokio::time::Instant {
        tokio::time::Instant::now()
    }
}

/// Keep real socket/storage IO from causing paused Tokio to jump to a failure guard.
pub struct ExplicitTimeGuard {
    release: Option<std::sync::mpsc::Sender<()>>,
    task: Option<tokio::task::JoinHandle<Result<(), std::sync::mpsc::RecvTimeoutError>>>,
}
impl ExplicitTimeGuard {
    pub fn new(bound: Duration) -> Self {
        let (release, released) = std::sync::mpsc::channel();
        let task = tokio::task::spawn_blocking(move || released.recv_timeout(bound));
        Self {
            release: Some(release),
            task: Some(task),
        }
    }

    pub async fn finish(mut self) -> super::TestResult<()> {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        self.task
            .take()
            .ok_or("explicit time guard task absent")?
            .await??;
        Ok(())
    }
}
impl Drop for ExplicitTimeGuard {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}

/// Advance the timer only between dispatches. An active real IO call must settle
/// before another tick; otherwise virtual ticks could expire its causal guard.
pub async fn progress_worker(
    passes: &mut tokio::sync::watch::Receiver<usize>,
) -> Result<(), tokio::sync::watch::error::RecvError> {
    let active_dispatches = *passes.borrow_and_update();
    if active_dispatches == 0 {
        tokio::time::advance(Duration::from_millis(250)).await;
    }
    passes.changed().await
}
