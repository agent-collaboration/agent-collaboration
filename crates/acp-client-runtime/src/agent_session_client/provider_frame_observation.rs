//! Bounded ACP frame-limit observation shared by connection and Session actors.

use super::{AtomicBool, Duration, Ordering};

#[cfg(any(test, feature = "test-observation"))]
static FRAME_LIMIT_EXCEEDED_COUNT: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(any(test, feature = "test-observation"))]
pub(crate) fn frame_limit_exceeded_count_for_test() -> u64 {
    FRAME_LIMIT_EXCEEDED_COUNT.load(Ordering::Relaxed)
}

#[derive(Debug, Default)]
pub(crate) struct ProviderFrameObservation {
    limit_exceeded: AtomicBool,
    limit_notification: tokio::sync::Notify,
}

impl ProviderFrameObservation {
    pub(super) fn record_limit_exceeded(&self) {
        #[cfg(any(test, feature = "test-observation"))]
        FRAME_LIMIT_EXCEEDED_COUNT.fetch_add(1, Ordering::Relaxed);
        self.limit_exceeded.store(true, Ordering::Relaxed);
        self.limit_notification.notify_one();
    }

    pub(crate) fn limit_was_exceeded(&self) -> bool {
        self.limit_exceeded.load(Ordering::Relaxed)
    }

    pub(crate) async fn wait_for_limit_exceeded(&self) -> bool {
        tokio::time::timeout(
            Duration::from_millis(100),
            self.limit_notification.notified(),
        )
        .await
        .is_ok()
    }
}
