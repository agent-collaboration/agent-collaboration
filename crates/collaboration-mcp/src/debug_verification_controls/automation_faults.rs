//! Causal fault holds on the debug Host's owner-only API socket.
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use collaboration_protocol::OperationId;
use collaboration_runtime::{
    AutomationFaultPoint, AutomationFaultSelector, DebugAutomationFaultController,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct FaultControlState {
    faults: Arc<DebugAutomationFaultController>,
    shutdown: CancellationToken,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArmRequest {
    point: AutomationFaultPoint,
    selector: AutomationFaultSelector,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArmIdentity {
    arm_id: OperationId,
}
pub(super) fn router(
    faults: Arc<DebugAutomationFaultController>,
    shutdown: CancellationToken,
) -> Router {
    Router::new()
        .route("/debug/automation/faults/arm", post(arm))
        .route("/debug/automation/faults/wait", post(wait))
        .route("/debug/automation/faults/release", post(release))
        .with_state(FaultControlState { faults, shutdown })
}
async fn arm(
    State(state): State<FaultControlState>,
    Json(request): Json<ArmRequest>,
) -> Result<Json<ArmIdentity>, StatusCode> {
    let arm_id = state
        .faults
        .arm(request.point, request.selector)
        .await
        .map_err(|_| StatusCode::CONFLICT)?;
    Ok(Json(ArmIdentity { arm_id }))
}
async fn wait(
    State(state): State<FaultControlState>,
    Json(request): Json<ArmIdentity>,
) -> Result<Json<collaboration_runtime::HeldAutomationFault>, StatusCode> {
    tokio::select! {
        _ = state.shutdown.cancelled() => Err(StatusCode::SERVICE_UNAVAILABLE),
        held = state.faults.wait_held(&request.arm_id) => held.map(Json).map_err(|_| StatusCode::CONFLICT),
    }
}
async fn release(
    State(state): State<FaultControlState>,
    Json(request): Json<ArmIdentity>,
) -> Result<StatusCode, StatusCode> {
    state
        .faults
        .release(&request.arm_id)
        .await
        .map_err(|_| StatusCode::CONFLICT)?;
    Ok(StatusCode::NO_CONTENT)
}
