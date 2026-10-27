//! Owner HTTP narrows authority once and carries typed Registry operations.
use crate::{
    AccessCaller, CallOutcome, CentralApplication, CentralPlatform, OwnerIdentity, RegistryKind,
    owner_registry_calls::{
        OwnerMove, OwnerPreviewAnswer, OwnerRegistryAnswer, OwnerRegistryCall,
        OwnerRegistryOperations, OwnerRetirement, OwnerRetirementAnswer, OwnerTakeover,
        OwnerTransferAnswer,
    },
};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use collaboration_central_admin_protocol::{OwnerOperationFailure, OwnerPreviewRefusal};
use fleet_sync_protocol::DatabaseId;
use std::sync::Arc;

async fn execute<Platform: CentralPlatform>(
    app: &CentralApplication<Platform>,
    caller: AccessCaller,
    database: DatabaseId,
    request: OwnerRegistryCall,
) -> Response {
    let owner = match OwnerIdentity::from_access(caller) {
        Ok(owner) => owner,
        Err(refusal) => return (StatusCode::UNAUTHORIZED, Json(refusal)).into_response(),
    };
    match app
        .call_from_owner::<RegistryKind, _>(
            owner,
            (),
            OwnerRegistryOperations { database },
            request,
        )
        .await
    {
        CallOutcome::Answered(OwnerRegistryAnswer::Preview(OwnerPreviewAnswer::Ready(preview))) => {
            Json(preview).into_response()
        }
        CallOutcome::Answered(OwnerRegistryAnswer::Preview(OwnerPreviewAnswer::NotFound)) => {
            Json(OwnerPreviewRefusal::NotFound).into_response()
        }
        CallOutcome::Answered(OwnerRegistryAnswer::Preview(OwnerPreviewAnswer::Retired)) => {
            Json(OwnerPreviewRefusal::Retired).into_response()
        }
        CallOutcome::Answered(OwnerRegistryAnswer::Transfer(answer)) => {
            let status = if matches!(&answer, OwnerTransferAnswer::Unavailable { .. }) {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::OK
            };
            (status, Json(answer)).into_response()
        }
        CallOutcome::Answered(OwnerRegistryAnswer::Retirement(answer)) => {
            let status = if matches!(&answer, OwnerRetirementAnswer::Unavailable { .. }) {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::OK
            };
            (status, Json(answer)).into_response()
        }
        CallOutcome::Failed(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(OwnerOperationFailure::ObjectUnavailable),
        )
            .into_response(),
    }
}
pub(crate) async fn preview<Platform: CentralPlatform>(
    State(app): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Path(database): Path<DatabaseId>,
) -> Response {
    execute(&app, caller, database, OwnerRegistryCall::Preview).await
}
pub(crate) async fn move_project<Platform: CentralPlatform>(
    State(app): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Path(database): Path<DatabaseId>,
    Json(request): Json<OwnerMove>,
) -> Response {
    execute(&app, caller, database, OwnerRegistryCall::Move(request)).await
}
pub(crate) async fn takeover<Platform: CentralPlatform>(
    State(app): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Path(database): Path<DatabaseId>,
    Json(request): Json<OwnerTakeover>,
) -> Response {
    execute(&app, caller, database, OwnerRegistryCall::Takeover(request)).await
}
pub(crate) async fn retire<Platform: CentralPlatform>(
    State(app): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Path(database): Path<DatabaseId>,
    Json(request): Json<OwnerRetirement>,
) -> Response {
    execute(&app, caller, database, OwnerRegistryCall::Retire(request)).await
}
