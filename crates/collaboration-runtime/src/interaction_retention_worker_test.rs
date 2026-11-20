use super::InteractionRetentionWorker;
use super::interaction_history_retention_tests::load_broker;
use super::retention_test_harness::{aging_push, insert_settled_push};
use crate::NativeGenerationGate;
use runtime_primitives::Clock;
use std::sync::Arc;

#[tokio::test]
async fn retention_worker_prunes_expired_push_records() {
    let clock = Arc::new(
        router_clock::ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
            1792432800000,
        ))
        .expect("fixed retention clock"),
    );
    let directory = tempfile::tempdir().expect("retention fixture");
    let gate = NativeGenerationGate::default();
    let broker = load_broker(directory.path(), &gate)
        .await
        .expect("interaction broker");
    let interactions = broker.interaction_store();
    let now_ms = clock.now().unix_millis();
    // One millisecond past the two-calendar-month cutoff.
    let created_at = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now_ms)
        .expect("valid retention clock")
        .checked_sub_months(chrono::Months::new(2))
        .expect("valid calendar cutoff")
        - chrono::Duration::milliseconds(1);
    let push_id = insert_settled_push(&interactions, aging_push(created_at)).await;

    let removed = InteractionRetentionWorker::new(Arc::clone(&broker), clock)
        .prune_batch(now_ms)
        .await;

    assert_eq!(removed, 1);
    assert!(
        interactions
            .get_push_record(&push_id)
            .await
            .expect("read pruned push")
            .is_none()
    );
}
