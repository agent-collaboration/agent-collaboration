use super::{DueBatchSnapshot, DueBatchState};
use crate::engine_alarm_decider;
use central_storage::EngineAlarmTimes;
use std::sync::{Arc, Barrier};

#[test]
fn the_spacing_boundary_and_backwards_clock_preserve_the_claim() {
    let state = DueBatchState::default();
    let spacing = chrono::Duration::seconds(30);
    assert_eq!(
        state.snapshot(),
        DueBatchSnapshot {
            last_start_unix_millis: None,
            leftovers_pending: false
        }
    );
    assert!(state.try_claim(-1, spacing).unwrap());
    assert!(!state.try_claim(-2, spacing).unwrap());
    assert!(!state.try_claim(29_998, spacing).unwrap());
    assert!(state.try_claim(29_999, spacing).unwrap());
    assert!(state.finish(29_999, true));
    assert_eq!(
        state.snapshot(),
        DueBatchSnapshot {
            last_start_unix_millis: Some(29_999),
            leftovers_pending: true
        }
    );
}

#[test]
fn simultaneous_events_claim_exactly_one_due_batch() {
    let state = Arc::new(DueBatchState::default());
    let spacing = chrono::Duration::seconds(30);
    assert!(state.try_claim(0, spacing).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    #[expect(
        clippy::needless_collect,
        reason = "every thread must be spawned before the barrier opens"
    )]
    let threads = (0..2)
        .map(|_| {
            let state = state.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                state.try_claim(31_000, spacing).unwrap()
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let claimed = threads
        .into_iter()
        .map(|thread| usize::from(thread.join().unwrap()))
        .sum::<usize>();
    assert_eq!(claimed, 1);
}

#[test]
fn a_late_old_batch_cannot_clear_the_newest_batchs_leftovers() {
    let state = DueBatchState::default();
    let spacing = chrono::Duration::seconds(30);
    assert!(state.try_claim(0, spacing).unwrap());
    assert!(state.try_claim(31_000, spacing).unwrap());
    assert!(state.finish(31_000, true));
    assert!(!state.finish(0, false));
    assert!(state.snapshot().leftovers_pending);
}

#[test]
fn a_prepared_publisher_uses_the_latest_batch_floor() {
    let state = Arc::new(DueBatchState::default());
    let decide = engine_alarm_decider(state.clone());
    let times = EngineAlarmTimes {
        earliest_on_grid_due_unix_millis: Some(35_000),
        earliest_legacy_due_unix_millis: None,
    };
    assert!(state.try_claim(0, chrono::Duration::seconds(30)).unwrap());
    assert!(
        state
            .try_claim(31_000, chrono::Duration::seconds(30))
            .unwrap()
    );
    assert_eq!(decide(times, 32_000, &mut |_| {}), Ok(Some(61_000)));
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn a_claim_waits_between_the_decision_and_real_watch_publication() {
    let state = Arc::new(DueBatchState::default());
    assert!(state.try_claim(0, chrono::Duration::seconds(30)).unwrap());
    let decide = engine_alarm_decider(state.clone());
    let times = EngineAlarmTimes {
        earliest_on_grid_due_unix_millis: Some(35_000),
        earliest_legacy_due_unix_millis: None,
    };
    let (alarm, retained) = tokio::sync::watch::channel(None::<i64>);
    let release = Arc::new(Barrier::new(2));
    let (ready_send, ready) = tokio::sync::oneshot::channel();
    let publishing = {
        let decide = decide.clone();
        let alarm = alarm.clone();
        let release = release.clone();
        std::thread::spawn(move || {
            let mut ready = Some(ready_send);
            decide(times, 32_000, &mut |time| {
                ready.take().unwrap().send(time).unwrap();
                release.wait();
                alarm.send_replace(time);
            })
            .unwrap()
        })
    };
    assert_eq!(ready.await.unwrap(), Some(35_000));
    let (finished_send, mut finished) = tokio::sync::oneshot::channel();
    let claiming = {
        let state = state.clone();
        std::thread::spawn(move || {
            assert!(
                state
                    .try_claim(31_000, chrono::Duration::seconds(30))
                    .unwrap()
            );
            decide(times, 32_000, &mut |time| {
                alarm.send_replace(time);
            })
            .unwrap();
            finished_send.send(()).unwrap();
        })
    };
    let waited = tokio::select! {
        () = state.until_claim_waiting() => true,
        result = &mut finished => { result.unwrap(); false },
    };
    release.wait();
    if waited {
        finished.await.unwrap();
    }
    tokio::task::spawn_blocking(move || {
        publishing.join().unwrap();
        claiming.join().unwrap();
    })
    .await
    .unwrap();
    assert!(
        waited,
        "claim crossed the decision/publication critical section"
    );
    assert_eq!(*retained.borrow(), Some(61_000));
}
