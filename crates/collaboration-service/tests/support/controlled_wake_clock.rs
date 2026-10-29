//! Explicit UTC advances drive wake policy; paused Tokio drives the unchanged worker tick.
use chrono::{DateTime, Utc};
use collaboration_service::SubscriptionClock;
use std::{sync::Mutex, time::Duration};

pub struct ControlledWakeClock {
    now: Mutex<DateTime<Utc>>,
}
impl ControlledWakeClock {
    pub fn new() -> Self {
        Self {
            now: Mutex::new(DateTime::from_timestamp(1_800_000_000, 0).unwrap()),
        }
    }

    pub async fn advance(&self, duration: Duration) {
        *self.now.lock().unwrap() += chrono::Duration::from_std(duration).unwrap();
        tokio::time::advance(duration).await;
    }
}
impl SubscriptionClock for ControlledWakeClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().unwrap()
    }
    fn monotonic_now(&self) -> tokio::time::Instant {
        tokio::time::Instant::now()
    }
}

/// Keep real socket/storage IO from causing paused Tokio to jump to a failure guard.
pub struct ExplicitTimeGuard {
    release: Option<std::sync::mpsc::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl ExplicitTimeGuard {
    pub fn new(bound: Duration) -> Self {
        let (release, released) = std::sync::mpsc::channel();
        let task = tokio::task::spawn_blocking(move || {
            released
                .recv_timeout(bound)
                .expect("wake proof exceeded its existing receive bound");
        });
        Self {
            release: Some(release),
            task: Some(task),
        }
    }

    pub async fn finish(mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        self.task.take().unwrap().await.unwrap();
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
pub async fn progress_worker(passes: &mut tokio::sync::watch::Receiver<usize>) {
    let active_dispatches = *passes.borrow_and_update();
    if active_dispatches == 0 {
        tokio::time::advance(Duration::from_millis(250)).await;
    }
    passes
        .changed()
        .await
        .expect("wake worker stopped before its next checkpoint");
}
