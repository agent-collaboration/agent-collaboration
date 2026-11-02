//! Owner selection must not replace the retained Local wait's cursor or deadline.
use automation_storage::{AutomationStore, WakeCreate};
use collaboration_protocol::{OperationId, SavedMessage, WakeWaitOutcome, WakeWaitRequest};
use collaboration_service::{WakeWaitResponse, collaboration_application::WakeOperations};
use futures_util::poll;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, Semaphore, oneshot};

type ProofResult<Value = ()> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test(start_paused = true)]
async fn retained_local_wait_keeps_its_cursor_after_delayed_owner_selection() -> ProofResult {
    // SQLite uses a dedicated thread. Hold automatic clock advancement until all owned
    // database work retires; only the explicit advances below move the test clock.
    let (release_clock, clock_held) = std::sync::mpsc::channel::<()>();
    let (clock_started, clock_ready) = oneshot::channel();
    let clock_holder = tokio::task::spawn_blocking(move || {
        let _ = clock_started.send(());
        let _ = clock_held.recv();
    });
    clock_ready.await?;

    let directory = tempfile::tempdir()?;
    let store = Arc::new(Mutex::new(
        AutomationStore::open(&directory.path().join("automation.sqlite")).await?,
    ));
    let proof = async {
        // Arrange a retained, active Local Wake, with no native backend or dispatcher.
        let message: SavedMessage = serde_json::from_value(serde_json::json!({
            "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"inert-wait-target"},
            "content":{"kind":"humanUser","text":"Retained Local reminder"},"delivery":"queue"
        }))?;
        let wake = store.lock().await.create_wakeup(&WakeCreate {
            requested_by: collaboration_protocol::WakeCreator::Owner {},
            replace: false,
            operation_id: OperationId::generate(),
            message,
            timing: agent_automation::TimingRule::After { seconds: 3600 },
            expiry: agent_automation::ExpiryRule::None,
            now_ms: chrono::Utc::now().timestamp_millis(),
        }).await?;
        let service = "00000000-0000-4000-8000-000000000001".to_owned().try_into()?;
        let operations = WakeOperations::new(&service, Some(&store))
            .with_wait_capacity(&Arc::new(Semaphore::new(1)));
        let selection_blocked = store.lock().await;
        let mut wait = Box::pin(operations.wake_wait_until_first_fire(WakeWaitRequest {
            wakeup_id: wake.definition.wakeup_id.clone(),
            after: None,
            timeout_seconds: Some(1),
        }));

        // Act: enqueue owner selection, then spend half a second behind its store lock.
        if !poll!(wait.as_mut()).is_pending() {
            return Err("wait did not retain the blocked owner-selection read".into());
        }
        tokio::time::advance(Duration::from_millis(500)).await;
        drop(selection_blocked);

        // Tokio's mutex is FIFO. Polling the retained future first puts the selection
        // ahead of this marker and the Local subscription read behind it.
        let selection_finished = tokio::select! {
            biased;
            result = &mut wait => return Err(format!("owner selection ended the Local wait: {result:?}").into()),
            marker = store.lock() => marker,
        };
        drop(selection_finished);
        // The second marker follows subscription initialization and blocks the next
        // observation read. The Local wait now owns a real retained event cursor.
        let cursor_initialized = tokio::select! {
            biased;
            result = &mut wait => return Err(format!("Local wait ended before cursor initialization: {result:?}").into()),
            marker = store.lock() => marker,
        };

        // Assert: the old outer deadline expires now, but the Local wait's own deadline
        // still has half a second remaining. It must retain its future and cursor.
        tokio::time::advance(Duration::from_millis(500)).await;
        if let std::task::Poll::Ready(result) = poll!(wait.as_mut()) {
            return Err(format!("outer deadline replaced the retained Local wait: {result:?}").into());
        }
        tokio::time::advance(Duration::from_millis(500)).await;
        let result = wait.await?;
        drop(cursor_initialized);
        let WakeWaitResponse::Local { value } = result else {
            return Err("retained Local Wake changed ownership".into());
        };
        if value.wakeup_id != wake.definition.wakeup_id
            || !matches!(value.outcome, WakeWaitOutcome::TimedOut)
            || value.cursor.is_empty()
        {
            return Err("Local deadline lost its Wake ID, timeout, or retained cursor".into());
        }
        let cursor: serde_json::Value = serde_json::from_str(&value.cursor)?;
        if cursor[1] != serde_json::json!(service) || cursor[2] != "automation-events" {
            return Err("Local wait fabricated a cursor outside its retained event collection".into());
        }
        Ok(())
    }.await;

    // Retire database and clock owners even when an assertion reports a wrong build.
    Arc::try_unwrap(store)
        .map_err(|_| "Local wait retained SQLite after completion")?
        .into_inner()
        .close()
        .await?;
    directory.close()?;
    drop(release_clock);
    clock_holder.await?;
    proof
}
