//! R4 under overlapping due batches on the native host: batch A passes the
//! attempt check with no record, batch B (31 s later) records the subject's
//! first failure, then A's corrupt load reports its own failure. A's failure
//! arrives before B's next try, so it counts for nothing (the pure attempt
//! transition decides under the record's guard).
use super::super::support::*;
use super::engine_probe_support::*;
use collaboration_central_core::PausePoint;
use router_clock::RuntimeClock;

#[tokio::test]
async fn a_failure_from_an_overlapping_batch_never_counts_before_the_next_try() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let due = fixture.clock.utc_now() + chrono::Duration::minutes(5);
    let (item, group) = (probe_id(13090), probe_id(13091));
    seed_probe(&runtime, item, group, Some(due), "corrupt").await;
    raw(
        &runtime,
        "UPDATE engine_probe_items SET state='damaged' WHERE item_id=?",
        vec![text(item)],
    )
    .await;
    // Batch A stops after its attempt check saw no record.
    let mut held = runtime
        .pause_points()
        .hold_engine(PausePoint::AfterEngineAttemptGate, &item.to_string());
    fixture.set_utc(due).await;
    let batch_a = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });
    held.until_held().await;

    // Batch B, past the floor, records the first failure.
    fixture.set_utc_while_a_batch_is_held(due + chrono::Duration::seconds(31));
    runtime.after_event().await.unwrap();
    assert_eq!(
        attempt(&runtime, item).await,
        vec![vec![text("failing"), integer(1), text("corrupt_row")]]
    );

    // A's corrupt load now reports its failure, before B's next try.
    held.release();
    batch_a.await.unwrap().unwrap();

    assert_eq!(
        attempt(&runtime, item).await,
        vec![vec![text("failing"), integer(1), text("corrupt_row")]],
        "a failure seen before the next try must not count as the second"
    );
    fixture.host.close().await;
}
