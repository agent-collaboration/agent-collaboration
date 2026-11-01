//! Causal notification clock steps on the debug Host's owner-only HTTP socket.
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use collaboration_service::SubscriptionDeliveryService;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct ClockControlState {
    clock: SubscriptionDeliveryService,
    shutdown: CancellationToken,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AdvanceClock {
    now_ms: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClockAdvanced {
    now_ms: i64,
    readers_completed: usize,
}
pub(super) fn router(clock: SubscriptionDeliveryService, shutdown: CancellationToken) -> Router {
    Router::new()
        .route("/debug/notifications/clock/advance", post(advance))
        .with_state(ClockControlState { clock, shutdown })
}
async fn advance(
    State(state): State<ClockControlState>,
    Json(request): Json<AdvanceClock>,
) -> Result<Json<ClockAdvanced>, (StatusCode, Json<message_board::BoardError>)> {
    let readers_completed = tokio::select! {
        _ = state.shutdown.cancelled() => return Err((StatusCode::SERVICE_UNAVAILABLE, Json(message_board::BoardError::board_unavailable()))),
        result = state.clock.debug_advance_notification_clock_millis(request.now_ms) => result.map_err(|error| {
            let status = if error.kind == message_board::BoardFailureKind::BoardUnavailable { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::BAD_REQUEST };
            (status, Json(error))
        })?,
    };
    Ok(Json(ClockAdvanced {
        now_ms: request.now_ms,
        readers_completed,
    }))
}
