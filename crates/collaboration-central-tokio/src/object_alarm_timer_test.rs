#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions fail these tests while timer errors propagate through Result."
)]
use super::ObjectAlarmTimer;
use collaboration_central_core::ObjectAlarm;
use runtime_primitives::{Clock, SyncTime, WakeScheduler};
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};
use std::time::Duration;

/// Business time the test moves by hand, independent of Tokio's paused time.
#[derive(Clone, Default)]
struct ManualClock(Arc<AtomicI64>);
impl ManualClock {
    fn set(&self, millis: i64) {
        self.0.store(millis, Ordering::SeqCst);
    }
}
impl Clock for ManualClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}

fn at(millis: i64) -> SyncTime {
    SyncTime::from_unix_millis(millis)
}

#[tokio::test(start_paused = true)]
async fn lowering_never_postpones_an_earlier_alarm() -> Result<(), Box<dyn std::error::Error>> {
    // Arrange
    let timer = ObjectAlarmTimer::new(Arc::new(ManualClock::default()));
    timer.set_exact(Some(at(100))).await?;

    // Act
    timer.wake_at(at(200)).await?;
    let after_later_lower = timer.stored().await?;
    timer.wake_at(at(50)).await?;

    // Assert
    assert_eq!(after_later_lower, Some(at(100)));
    assert_eq!(timer.stored().await?, Some(at(50)));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn exact_set_moves_the_alarm_later_and_deletes_it() -> Result<(), Box<dyn std::error::Error>>
{
    // Arrange
    let timer = ObjectAlarmTimer::new(Arc::new(ManualClock::default()));
    timer.wake_at(at(50)).await?;

    // Act
    timer.set_exact(Some(at(300))).await?;
    let moved = timer.stored().await?;
    timer.set_exact(None).await?;

    // Assert: a lower-only timer would have kept 50 here.
    assert_eq!(moved, Some(at(300)));
    assert_eq!(timer.stored().await?, None);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn delivery_consumes_the_stored_alarm() -> Result<(), Box<dyn std::error::Error>> {
    // Arrange
    let timer = ObjectAlarmTimer::new(Arc::new(ManualClock::default()));
    timer.set_exact(Some(at(300))).await?;

    // Act: paused Tokio time advances to the trigger once the runtime is idle.
    let fired = timer.wait().await;

    // Assert
    assert_eq!(fired, at(300));
    assert_eq!(timer.stored().await?, None);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn deleted_alarm_never_fires() -> Result<(), Box<dyn std::error::Error>> {
    // Arrange
    let timer = Arc::new(ObjectAlarmTimer::new(Arc::new(ManualClock::default())));
    timer.set_exact(Some(at(300))).await?;
    timer.set_exact(None).await?;

    // Act: virtual time runs far past the deleted deadline.
    let fired = tokio::time::timeout(Duration::from_secs(3600), timer.wait()).await;

    // Assert
    assert!(fired.is_err(), "a deleted alarm fired at {fired:?}");
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn clock_change_rederives_the_trigger_without_moving_the_alarm()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange: an alarm an hour of business time away.
    let clock = ManualClock::default();
    let timer = ObjectAlarmTimer::new(Arc::new(clock.clone()));
    timer.set_exact(Some(at(3_600_000))).await?;

    // Act: business time jumps past the deadline; Tokio time does not move.
    clock.set(3_600_001);
    timer.clock_changed().await?;
    let stored_before_delivery = timer.stored().await?;
    // One virtual second, far short of the hour the old trigger was set for.
    let fired = tokio::time::timeout(Duration::from_secs(1), timer.wait()).await;

    // Assert: the same alarm fires at once; nothing earlier was manufactured.
    assert_eq!(stored_before_delivery, Some(at(3_600_000)));
    assert_eq!(fired.ok(), Some(at(3_600_000)));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn clock_change_with_no_alarm_creates_none() -> Result<(), Box<dyn std::error::Error>> {
    // Arrange
    let clock = ManualClock::default();
    let timer = ObjectAlarmTimer::new(Arc::new(clock.clone()));

    // Act
    clock.set(10_000);
    timer.clock_changed().await?;

    // Assert: the old SetTime control lowered the alarm to now here.
    assert_eq!(timer.stored().await?, None);
    Ok(())
}
