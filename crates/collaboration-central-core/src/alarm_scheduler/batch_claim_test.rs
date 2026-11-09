use super::{BatchClaim, ClaimDeclined};
use runtime_primitives::SyncTime;

fn at(seconds: i64) -> SyncTime {
    SyncTime::from_unix_millis(seconds * 1000)
}

#[test]
fn a_second_batch_waits_one_cadence_after_the_first_started() {
    // Arrange
    let claim = BatchClaim::default();
    let first = claim.try_claim(at(100));
    drop(first);

    // Act
    let inside_cadence = claim.try_claim(at(129)).map(drop);
    let at_cadence = claim.try_claim(at(130)).map(drop);

    // Assert: the cadence counts from the batch's start, 30 s.
    assert_eq!(
        inside_cadence,
        Err(ClaimDeclined::Cadence { until: at(130) })
    );
    assert_eq!(at_cadence, Ok(()));
}

#[test]
fn a_running_batch_blocks_every_other_claim_until_its_guard_drops() {
    // Arrange
    let claim = BatchClaim::default();
    let running = claim.try_claim(at(100));

    // Act: another event long after the cadence, while the first still runs.
    let while_running = claim.try_claim(at(140)).map(drop);
    drop(running);
    let after_release = claim.try_claim(at(140)).map(drop);

    // Assert
    assert_eq!(while_running, Err(ClaimDeclined::Running));
    assert_eq!(after_release, Ok(()));
}

#[test]
fn a_claim_older_than_twice_the_batch_deadline_is_free() {
    // Arrange: a guard that never dropped (leaked), as after a lost task.
    let claim = BatchClaim::default();
    std::mem::forget(claim.try_claim(at(100)));

    // Act
    let at_limit = claim.try_claim(at(219)).map(drop);
    let past_limit = claim.try_claim(at(220)).map(drop);

    // Assert: 2 x 60 s after it started, the stuck claim stops blocking batches.
    assert_eq!(at_limit, Err(ClaimDeclined::Running));
    assert_eq!(past_limit, Ok(()));
}

#[test]
fn the_next_batch_time_follows_the_last_start() {
    // Arrange
    let claim = BatchClaim::default();
    let before_any = claim.next_batch_at();

    // Act
    drop(claim.try_claim(at(100)));

    // Assert
    assert_eq!(before_any, None);
    assert_eq!(claim.next_batch_at(), Some(at(130)));
}
