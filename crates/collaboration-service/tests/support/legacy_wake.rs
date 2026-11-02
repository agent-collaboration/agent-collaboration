//! Arrange an actual pre-upgrade local Wake; public create always belongs to central.
use collaboration_protocol::{WakeSendRequest, WakeSnapshot, WakeupId};
use std::sync::Arc;
type ProofResult<Value> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;

pub async fn seed(
    store: &Arc<tokio::sync::Mutex<automation_storage::AutomationStore>>,
    request: WakeSendRequest,
) -> ProofResult<WakeupId> {
    let record = store
        .lock()
        .await
        .create_wakeup(&automation_storage::WakeCreate {
            operation_id: request.operation_id,
            requested_by: request.requested_by,
            replace: request.replace,
            message: request.message,
            timing: serde_json::from_value(serde_json::to_value(request.timing)?)?,
            expiry: serde_json::from_value(serde_json::to_value(request.expiry)?)?,
            now_ms: chrono::Utc::now().timestamp_millis(),
        })
        .await?;
    Ok(record.definition.wakeup_id)
}

pub fn snapshot(response: collaboration_client::WakeReadResponse) -> ProofResult<WakeSnapshot> {
    match response {
        collaboration_client::WakeReadResponse::Local { value } => Ok(value),
        _ => Err("seeded pre-upgrade Wake did not retain local ownership".into()),
    }
}
