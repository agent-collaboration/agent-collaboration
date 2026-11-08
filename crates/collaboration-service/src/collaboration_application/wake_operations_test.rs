use super::WakeOperations;
use automation_storage::AutomationStore;
use collaboration_protocol::{
    OperationId, UuidIdentity, WakeMutationRequest, WakeSendRequest, WakeWaitOutcome,
    WakeWaitRequest,
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, Semaphore};

fn service() -> UuidIdentity {
    "00000000-0000-4000-8000-000000000001"
        .to_owned()
        .try_into()
        .unwrap()
}

/// A wake whose first fire is an hour away, so only a mutation can answer a wait.
fn hour_away_wake() -> WakeSendRequest {
    serde_json::from_value(json!({
        "operationId": OperationId::generate(),
        "requestedBy": {"kind": "owner"},
        "replace": false,
        "message": {
            "target": {"endpoint": {"serviceId": service(), "endpointId": "codex-local"}, "sessionId": "waiting-reader"},
            "content": {"kind": "humanUser", "text": "first-fire wait proof"},
            "delivery": "auto"
        },
        "timing": {"kind": "at", "at": (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)},
        "expiry": {"kind": "at", "at": (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)}
    }))
    .unwrap()
}

async fn wake_operations(directory: &std::path::Path) -> Arc<WakeOperations> {
    let store = Arc::new(Mutex::new(
        AutomationStore::open(&directory.join("automation.sqlite"))
            .await
            .unwrap(),
    ));
    let permits = Arc::new(Semaphore::new(1));
    Arc::new(WakeOperations::new(&service(), Some(&store)).with_wait_capacity(&permits))
}

/// Bounds a wait the test expects to end on a commit or a deadline; it only stops a hang.
const HANG_GUARD: Duration = Duration::from_secs(10);

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

// SQLite answers on background threads, so the clock runs until the waiter is parked.
#[tokio::test]
async fn a_first_fire_wait_answers_the_commit_that_lands_after_its_empty_read() {
    let directory = tempfile::tempdir().unwrap();
    let wakes = wake_operations(directory.path()).await;
    let wake = wakes.wake_send_local(hour_away_wake()).await.unwrap();
    let observation = wakes.wait_observation();
    let mut gate = observation.hold_before_next_sleep();
    let waiter = spawn_wait(&wakes, wake.definition.wakeup_id.clone(), 60);
    gate.reached().await;

    // The pause commits after the wait's empty read and before it starts waiting.
    wakes
        .wake_pause_local(WakeMutationRequest {
            operation_id: OperationId::generate(),
            wakeup_id: wake.definition.wakeup_id.clone(),
        })
        .await
        .unwrap();
    gate.release();
    let answered = tokio::time::timeout(HANG_GUARD, waiter)
        .await
        .expect("the commit, not the 60 s deadline, ends the wait")
        .unwrap()
        .unwrap();

    assert!(
        matches!(answered.outcome, WakeWaitOutcome::Paused),
        "{:?}",
        answered.outcome
    );
    let snapshot = observation.snapshot();
    assert_eq!(snapshot.reads, vec!["wake_wait.next_changes"; 2]);
    assert_eq!(snapshot.sleeps, 1);
}

// The clock is paused only once the waiter is parked, with no store read in flight.
#[tokio::test]
async fn a_first_fire_wait_with_nothing_committed_reads_once_then_once_at_its_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let wakes = wake_operations(directory.path()).await;
    let wake = wakes.wake_send_local(hour_away_wake()).await.unwrap();
    let observation = wakes.wait_observation();
    let mut gate = observation.hold_before_next_sleep();
    let before_wait = tokio::time::Instant::now();
    let waiter = spawn_wait(&wakes, wake.definition.wakeup_id.clone(), 30);
    gate.reached().await;
    tokio::time::pause();
    let at_gate = observation.snapshot();
    let parked_at = tokio::time::Instant::now();

    gate.release();
    let waited = waiter.await.unwrap().unwrap();

    assert!(
        matches!(waited.outcome, WakeWaitOutcome::TimedOut),
        "{:?}",
        waited.outcome
    );
    assert_waited_for_the_caller_deadline(
        &at_gate,
        before_wait,
        parked_at,
        "wake_wait.next_changes",
    );
    let snapshot = observation.snapshot();
    assert_eq!(
        snapshot.reads,
        vec!["wake_wait.next_changes"; 2],
        "one read before the wait and one at the deadline, none in between"
    );
    assert_eq!(snapshot.sleeps, 1);
}

/// The waiter armed its timer for the caller's 30 s bound, which began before its setup:
/// no earlier than 30 s after the call and no later than 30 s after it parked. Only the
/// paused clock then moved, straight to that time, and it had read once before parking.
pub(crate) fn assert_waited_for_the_caller_deadline(
    at_gate: &collaboration_runtime::LoopSnapshot,
    before_wait: tokio::time::Instant,
    parked_at: tokio::time::Instant,
    read: &'static str,
) {
    let bound = Duration::from_secs(30);
    let wakes_at = at_gate
        .waiter_wakes_at
        .expect("the waiter reported its timer");
    assert!(
        wakes_at >= before_wait + bound && wakes_at <= parked_at + bound,
        "the waiter armed {:?} after the call, not the caller's {bound:?} bound",
        wakes_at.duration_since(before_wait)
    );
    assert_eq!(
        at_gate.reads,
        vec![read],
        "one read before the waiter parked"
    );
    let woke_at = tokio::time::Instant::now();
    assert!(
        woke_at >= wakes_at && woke_at.duration_since(wakes_at) < Duration::from_millis(2),
        "the paused clock moved straight to the armed time, not past it"
    );
}
