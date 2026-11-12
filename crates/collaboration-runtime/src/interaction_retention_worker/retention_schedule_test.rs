//! The retention worker on real stores: it arms one timer at the first moment a delete rule
//! allows a row to go, deletes nothing before it, and a commit that makes nothing due
//! deletes nothing.
//!
//! Real store operations remain asynchronous; only the manual clock advances deadlines.
use super::retention_test_harness::*;
use chrono::DateTime;
use runtime_primitives::Clock;
use sqlx::Connection;
use tokio_util::sync::CancellationToken;

async fn assert_deleted_at_its_cutoff(stamped: &str, wall: &str, expected_due: &str) {
    let directory = tempfile::tempdir().unwrap();
    let interactions = open_interactions(directory.path()).await;
    insert_route(&interactions, "aging", at(stamped)).await;

    let worker = RunningWorker::start(Some(interactions.clone()), None, at(wall)).await;

    let parked = worker.snapshot();
    assert_eq!(
        armed(&parked),
        Some(at(expected_due)),
        "one timer at the strict cutoff"
    );
    assert!(
        prunes(&parked).is_empty(),
        "nothing was due before the cutoff"
    );
    assert_eq!(route_ids(directory.path()).await, vec!["aging"]);

    run_to_the_armed_time(&worker).await;

    let woke = worker.snapshot();
    let woke_at = DateTime::from_timestamp_millis(worker.clock.now().unix_millis()).unwrap();
    assert!(
        woke_at >= at(expected_due)
            && woke_at - at(expected_due) <= chrono::Duration::milliseconds(2),
        "the clock reached the cutoff and no further: {woke_at}"
    );
    assert_eq!(prunes(&woke), vec!["retention.interaction_records.prune"]);
    assert!(route_ids(directory.path()).await.is_empty());
    assert_eq!(
        armed(&woke),
        None,
        "with nothing left to age, no timer is held"
    );
    worker.stop().await;
}

#[tokio::test]
async fn a_row_goes_one_millisecond_after_exactly_two_calendar_months() {
    assert_deleted_at_its_cutoff(
        "2026-01-15T10:00:05.000Z",
        "2026-03-15T10:00:00.000Z",
        "2026-03-15T10:00:05.001Z",
    )
    .await;
}

#[tokio::test]
async fn a_month_end_row_waits_until_the_cutoff_leaves_the_short_month() {
    // The cutoff stands at 2025-12-28 for the whole of 2026-02-28.
    assert_deleted_at_its_cutoff(
        "2025-12-31T12:00:00.000Z",
        "2026-02-28T22:00:00.000Z",
        "2026-03-01T00:00:00.000Z",
    )
    .await;
}

#[tokio::test]
async fn a_commit_that_makes_nothing_due_reads_due_times_and_deletes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let interactions = open_interactions(directory.path()).await;
    let wall = at("2026-06-15T12:00:00.000Z");
    insert_route(&interactions, "later", at("2026-04-15T13:00:00.000Z")).await;
    let worker = RunningWorker::start(Some(interactions.clone()), None, wall).await;
    let parked = worker.snapshot();

    // A freshly used route commits, too young to go.
    insert_route(&interactions, "fresh", wall).await;
    worker.observation.asleep_at_least(parked.sleeps + 1).await;

    let woke = worker.snapshot();
    let since_park = &woke.reads[parked.reads.len()..];
    assert!(
        !since_park.is_empty(),
        "the commit woke a pass that read the due time"
    );
    assert!(
        since_park.iter().all(|read| read.ends_with(".due")),
        "a commit that leaves no class due runs no DELETE: {since_park:?}"
    );
    assert_eq!(armed(&woke), armed(&parked));
    assert_eq!(route_ids(directory.path()).await, vec!["fresh", "later"]);
    worker.stop().await;
}

#[tokio::test]
async fn settling_a_pending_approval_arms_the_history_timer_at_its_due_time() {
    use super::interaction_history_retention_tests::{load_broker, session};
    let directory = tempfile::tempdir().unwrap();
    let gate = crate::NativeGenerationGate::default();
    let broker = load_broker(directory.path(), &gate).await.unwrap();
    let request: session_event_model::ApprovalRequest = serde_json::from_value(serde_json::json!({
        "requestId": "aging-approval", "title": "Run command", "options": [
            {"optionId": "allow-once", "label": "Allow once", "choice": {"effect": "allow", "scope": "once"}}
        ]
    }))
    .unwrap();
    let requester =
        serde_json::from_value(serde_json::to_value(session("requester").unwrap()).unwrap())
            .unwrap();
    let approver =
        serde_json::from_value(serde_json::json!({"kind": "human", "humanId": "retention-owner"}))
            .unwrap();
    let _resolution = broker
        .request_typed_approval(
            requester,
            approver,
            request,
            CancellationToken::new(),
            CancellationToken::new(),
            None,
        )
        .await
        .unwrap();
    let mut history = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(directory.path().join("interaction.sqlite"))
            .read_only(true),
    )
    .await
    .unwrap();
    let created_at: String = sqlx::query_scalar::<sqlx_turso::Turso, String>(
        "SELECT created_at FROM typed_interaction_history WHERE request_id='aging-approval'",
    )
    .fetch_one(&mut history)
    .await
    .unwrap();
    history.close().await.unwrap();
    let created_at = at(&created_at);
    let worker = RunningWorker::start(None, Some(broker.clone()), created_at).await;
    let parked = worker.snapshot();
    assert_eq!(armed(&parked), None, "a pending approval holds no timer");

    // Cancelling settles the approval through the broker: an interaction-store commit.
    broker
        .cancel_typed_approval("aging-approval", "retention proof")
        .await
        .unwrap();
    (worker.observation.asleep_at_least(parked.sleeps + 1)).await;

    let expected_due =
        sqlite_storage_maintenance::first_deletable_at(created_at, 2, created_at).unwrap();
    let woke = worker.snapshot();
    assert_eq!(
        armed(&woke).map(|due| due.timestamp_millis()),
        Some(expected_due.timestamp_millis()),
        "the settled record arms the timer at the first moment its rule allows"
    );
    run_to_the_armed_time(&worker).await;
    assert_eq!(
        prunes(&worker.snapshot()),
        vec!["retention.interaction_history.prune"]
    );
    assert!(
        broker
            .list_interactions()
            .await
            .iter()
            .all(|record| record.request_id() != "aging-approval")
    );
    worker.stop().await;
}
