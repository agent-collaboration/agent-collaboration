use super::GatedClock;
use runtime_primitives::{Clock, SyncTime};
use std::time::Duration;

fn clock() -> GatedClock {
    GatedClock::starting_at(SyncTime::from_unix_millis(1_000_000)).unwrap()
}

#[tokio::test]
async fn a_move_waits_for_a_held_drive() {
    let clock = clock();
    let drive = clock.hold_drive().await;
    let mut moved = std::pin::pin!(clock.advance(Duration::from_secs(1)));
    tokio::select! {
        biased;
        _ = &mut moved => panic!("the clock moved while a drive was held"),
        () = clock.until_move_waiting() => {}
    }
    assert_eq!(clock.now().unix_millis(), 1_000_000);
    drop(drive);
    moved.await.unwrap();
    assert_eq!(clock.now().unix_millis(), 1_001_000);
}

#[tokio::test]
async fn a_drive_waits_for_a_held_move() {
    let clock = clock();
    let held = clock.hold_move().await;
    let mut drive = std::pin::pin!(clock.hold_drive());
    tokio::select! {
        biased;
        _ = &mut drive => panic!("a drive started during a move"),
        () = clock.until_drive_waiting() => {}
    }
    held.advance(Duration::from_secs(1)).unwrap();
    drop(held);
    let _drive = drive.await;
    assert_eq!(clock.now().unix_millis(), 1_001_000);
}

#[tokio::test]
async fn a_drive_queues_behind_a_waiting_move() {
    let clock = clock();
    let first = clock.hold_drive().await;
    let mut moved = std::pin::pin!(clock.advance(Duration::from_secs(1)));
    tokio::select! {
        biased;
        _ = &mut moved => panic!("the clock moved while a drive was held"),
        () = clock.until_move_waiting() => {}
    }
    let mut second = std::pin::pin!(clock.hold_drive());
    tokio::select! {
        biased;
        _ = &mut second => panic!("a new drive overtook a waiting move"),
        () = clock.until_drive_waiting() => {}
    }
    drop(first);
    moved.await.unwrap();
    let _second = second.await;
    assert_eq!(clock.now().unix_millis(), 1_001_000);
}
