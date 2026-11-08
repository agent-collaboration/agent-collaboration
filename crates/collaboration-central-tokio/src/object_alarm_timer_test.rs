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

/// Park a waiter on the timer. On the test's current-thread runtime one yield
/// runs the spawned task until it awaits the alarm.
async fn parked_waiter(timer: &Arc<ObjectAlarmTimer>) -> tokio::task::JoinHandle<SyncTime> {
    let waiting = timer.clone();
    let handle = tokio::spawn(async move { waiting.wait().await });
    tokio::task::yield_now().await;
    handle
}

#[tokio::test(start_paused = true)]
async fn parked_waiter_fires_at_an_earlier_lower() -> Result<(), Box<dyn std::error::Error>> {
    // Arrange: a waiter already parked on an alarm 100 s away.
    let timer = Arc::new(ObjectAlarmTimer::new(Arc::new(ManualClock::default())));
    timer.set_exact(Some(at(100_000))).await?;
    let waiter = parked_waiter(&timer).await;

    // Act: a unit lowers the alarm to 10 s.
    timer.wake_at(at(10_000)).await?;
    let fired = tokio::time::timeout(Duration::from_secs(20), waiter).await;

    // Assert: a waiter that kept its old 100 s trigger would still be asleep.
    assert_eq!(fired.ok().transpose()?, Some(at(10_000)));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn parked_waiter_cannot_fire_at_a_bound_moved_later() -> Result<(), Box<dyn std::error::Error>>
{
    // Arrange: a waiter already parked on an alarm 10 s away.
    let timer = Arc::new(ObjectAlarmTimer::new(Arc::new(ManualClock::default())));
    timer.set_exact(Some(at(10_000))).await?;
    let waiter = parked_waiter(&timer).await;

    // Act: the recompute moves the alarm to 100 s.
    timer.set_exact(Some(at(100_000))).await?;
    let mut waiter = waiter;
    let before_new_bound = tokio::time::timeout(Duration::from_secs(20), &mut waiter).await;
    let at_new_bound = tokio::time::timeout(Duration::from_secs(100), &mut waiter).await;

    // Assert: nothing fires at the old 10 s bound; the moved alarm fires at 100 s.
    assert!(
        before_new_bound.is_err(),
        "fired at the old bound: {before_new_bound:?}"
    );
    assert_eq!(at_new_bound.ok().transpose()?, Some(at(100_000)));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn clock_change_wakes_a_parked_waiter() -> Result<(), Box<dyn std::error::Error>> {
    // Arrange: a waiter parked on an alarm an hour of business time away.
    let clock = ManualClock::default();
    let timer = Arc::new(ObjectAlarmTimer::new(Arc::new(clock.clone())));
    timer.set_exact(Some(at(3_600_000))).await?;
    let waiter = parked_waiter(&timer).await;

    // Act: business time passes the deadline; Tokio time does not.
    clock.set(3_600_001);
    timer.clock_changed().await?;
    let fired = tokio::time::timeout(Duration::from_secs(1), waiter).await;

    // Assert: a trigger update that never notified the parked wait would leave it asleep.
    assert_eq!(fired.ok().transpose()?, Some(at(3_600_000)));
    Ok(())
}
