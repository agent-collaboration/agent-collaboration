use super::WakeOperations;
use automation_storage::AutomationStore;
use collaboration_protocol::{
    OperationId, UuidIdentity, WakeMutationRequest, WakeSendRequest, WakeWaitOutcome,
    WakeWaitRequest,
};
use router_clock::{ManualClock, RuntimeClock};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, Semaphore};

fn service() -> UuidIdentity {
    "00000000-0000-4000-8000-000000000001"
        .to_owned()
        .try_into()
        .unwrap()
}

fn hour_away_wake(clock: &ManualClock) -> WakeSendRequest {
    serde_json::from_value(json!({
        "operationId": OperationId::generate(), "requestedBy": {"kind": "owner"},
        "replace": false,
        "message": {
            "target": {"endpoint": {"serviceId": service(), "endpointId": "codex-local"}, "sessionId": "waiting-reader"},
            "content": {"kind": "humanUser", "text": "first-fire wait proof"}, "delivery": "auto"
        },
        "timing": {"kind": "at", "at": (clock.utc_now() + chrono::Duration::hours(1)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)},
        "expiry": {"kind": "at", "at": (clock.utc_now() + chrono::Duration::hours(2)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)}
    })).unwrap()
}

async fn wake_operations(directory: &std::path::Path) -> (Arc<WakeOperations>, Arc<ManualClock>) {
    let store = Arc::new(Mutex::new(
        AutomationStore::open(&directory.join("automation.sqlite"))
            .await
            .unwrap(),
    ));
    let permits = Arc::new(Semaphore::new(1));
    let clock = Arc::new(ManualClock::new("2001-02-03T04:05:06Z".parse().unwrap()));
    let wakes = Arc::new(
        WakeOperations::new(clock.clone(), &service(), Some(&store)).with_wait_capacity(&permits),
    );
    (wakes, clock)
}

fn spawn_wait(
    wakes: &Arc<WakeOperations>,
    wakeup_id: agent_automation::WakeupId,
    timeout_seconds: u32,
) -> tokio::task::JoinHandle<Result<collaboration_protocol::WakeWaitResult, super::WakeWaitFailure>>
{
    let wakes = wakes.clone();
    tokio::spawn(async move {
        wakes
            .wake_wait_local(WakeWaitRequest {
                wakeup_id,
                after: None,
                timeout_seconds: Some(timeout_seconds),
            })
            .await
    })
}

#[tokio::test]
async fn a_first_fire_wait_answers_the_commit_that_lands_after_its_empty_read() {
    let directory = tempfile::tempdir().unwrap();
    let (wakes, clock) = wake_operations(directory.path()).await;
    let wake = wakes.wake_send_local(hour_away_wake(&clock)).await.unwrap();
    let observation = wakes.wait_observation();
    let mut gate = observation.hold_before_next_sleep();
    let waiter = spawn_wait(&wakes, wake.definition.wakeup_id.clone(), 60);
    gate.reached().await;
    wakes
        .wake_pause_local(WakeMutationRequest {
            operation_id: OperationId::generate(),
            wakeup_id: wake.definition.wakeup_id.clone(),
        })
        .await
        .unwrap();
    gate.release();
    let answered = waiter.await.unwrap().unwrap();
    assert!(
        matches!(answered.outcome, WakeWaitOutcome::Paused),
        "{:?}",
        answered.outcome
    );
    assert_eq!(
        clock.utc_now(),
        "2001-02-03T04:05:06Z"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap()
    );
    let snapshot = observation.snapshot();
    assert_eq!(snapshot.reads, vec!["wake_wait.next_changes"; 2]);
    assert_eq!(snapshot.sleeps, 1);
}

#[tokio::test]
async fn a_first_fire_wait_with_nothing_committed_reads_once_then_once_at_its_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let (wakes, clock) = wake_operations(directory.path()).await;
    let wake = wakes.wake_send_local(hour_away_wake(&clock)).await.unwrap();
    let observation = wakes.wait_observation();
    let mut gate = observation.hold_before_next_sleep();
    let deadline = clock.monotonic_now() + Duration::from_secs(30);
    let waiter = spawn_wait(&wakes, wake.definition.wakeup_id.clone(), 30);
    gate.reached().await;
    assert_eq!(observation.snapshot().waiter_wakes_at, Some(deadline));
    gate.release();
    clock.not_due_check_now(deadline).await;
    clock.advance(Duration::from_secs(29)).unwrap();
    clock.not_due_check_now(deadline).await;
    assert!(!waiter.is_finished());
    assert_eq!(observation.snapshot().reads, vec!["wake_wait.next_changes"]);
    clock.advance(Duration::from_secs(1)).unwrap();
    let waited = waiter.await.unwrap().unwrap();
    assert!(
        matches!(waited.outcome, WakeWaitOutcome::TimedOut),
        "{:?}",
        waited.outcome
    );
    let snapshot = observation.snapshot();
    assert_eq!(snapshot.reads, vec!["wake_wait.next_changes"; 2]);
    assert_eq!(snapshot.sleeps, 1);
    assert_eq!(clock.monotonic_now(), deadline);
}

#[tokio::test]
async fn a_wait_blocked_before_its_first_store_read_uses_the_same_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let (wakes, clock) = wake_operations(directory.path()).await;
    let wake = wakes.wake_send_local(hour_away_wake(&clock)).await.unwrap();
    let _held_store = wakes.store.as_ref().unwrap().lock().await;
    let deadline = clock.monotonic_now() + Duration::from_secs(30);
    let waiter = spawn_wait(&wakes, wake.definition.wakeup_id, 30);
    clock.not_due_check_now(deadline).await;
    clock.advance(Duration::from_secs(29)).unwrap();
    clock.not_due_check_now(deadline).await;
    assert!(!waiter.is_finished());
    clock.advance(Duration::from_secs(1)).unwrap();
    assert!(matches!(
        waiter.await.unwrap(),
        Err(super::WakeWaitFailure::Unavailable(_))
    ));
}
