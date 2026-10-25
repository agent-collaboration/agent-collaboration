//! The shared HTTP application narrows the owner before constructing operations.
use crate::{
    AccessCaller, CallOutcome, CentralApplication, CentralPlatform, CopyKind, FleetKind,
    OwnerIdentity,
    owner_operation_calls::{
        CopyDirectoryPage, CopyOperations, CopyOpsAnswer, CopyOpsCall, CopyPageLimit,
        CopyPageRequest, ReadCopyDirectoryPage,
    },
};
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use collaboration_central_admin_protocol::{
    CopyPageQuery, CopyRepairAnswer, OwnerOperationFailure,
};
use std::sync::Arc;
pub(crate) fn failed(failure: durable_steps::CallFailure) -> Response {
    let body = match failure {
        durable_steps::CallFailure::Retryable { reason } => {
            match fleet_sync_protocol::CopyUnavailableReason::decode_retry(
                &reason.code,
                &reason.detail,
            ) {
                Ok(fleet_sync_protocol::CopyUnavailableReason::StorageFull) => {
                    OwnerOperationFailure::CentralCopyFull
                }
                Ok(reason) => OwnerOperationFailure::CopyUnavailable { reason },
                Err(_) => OwnerOperationFailure::ObjectUnavailable,
            }
        }
        _ => OwnerOperationFailure::ObjectUnavailable,
    };
    (StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response()
}
pub(crate) async fn health<Platform: CentralPlatform>(
    State(app): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Path(database): Path<fleet_sync_protocol::DatabaseId>,
) -> Response {
    let owner = match OwnerIdentity::from_access(caller) {
        Ok(owner) => owner,
        Err(refusal) => return (StatusCode::UNAUTHORIZED, Json(refusal)).into_response(),
    };
    match app
        .call_from_owner::<CopyKind, _>(
            owner,
            database,
            CopyOperations { database },
            CopyOpsCall::Health,
        )
        .await
    {
        CallOutcome::Answered(CopyOpsAnswer::Health(health)) => Json(health).into_response(),
        CallOutcome::Answered(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(OwnerOperationFailure::ObjectUnavailable),
        )
            .into_response(),
        CallOutcome::Failed(failure) => failed(failure),
    }
}
pub(crate) async fn copies<Platform: CentralPlatform>(
    State(app): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Query(query): Query<CopyPageQuery>,
) -> Response {
    let owner = match OwnerIdentity::from_access(caller) {
        Ok(owner) => owner,
        Err(refusal) => return (StatusCode::UNAUTHORIZED, Json(refusal)).into_response(),
    };
    let limit = match CopyPageLimit::try_from(query.limit.unwrap_or(100)) {
        Ok(limit) => limit,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(OwnerOperationFailure::InvalidPageLimit),
            )
                .into_response();
        }
    };
    match app
        .call_from_owner::<FleetKind, _>(
            owner,
            (),
            ReadCopyDirectoryPage,
            CopyPageRequest {
                after: query.after,
                limit,
            },
        )
        .await
    {
        CallOutcome::Answered(page) => Json::<CopyDirectoryPage>(page).into_response(),
        CallOutcome::Failed(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(OwnerOperationFailure::ObjectUnavailable),
        )
            .into_response(),
    }
}
pub(crate) async fn repair<Platform: CentralPlatform>(
    State(app): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Path(database): Path<fleet_sync_protocol::DatabaseId>,
) -> Response {
    let owner = match OwnerIdentity::from_access(caller) {
        Ok(owner) => owner,
        Err(refusal) => return (StatusCode::UNAUTHORIZED, Json(refusal)).into_response(),
    };
    match app
        .call_from_owner::<CopyKind, _>(
            owner,
            database,
            CopyOperations { database },
            CopyOpsCall::Repair,
        )
        .await
    {
        CallOutcome::Answered(CopyOpsAnswer::Repaired) => {
            Json(CopyRepairAnswer::Repaired).into_response()
        }
        CallOutcome::Answered(CopyOpsAnswer::Retired) => {
            (StatusCode::GONE, Json(OwnerOperationFailure::Retired)).into_response()
        }
        CallOutcome::Answered(CopyOpsAnswer::Health(_) | CopyOpsAnswer::Migration(_)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(OwnerOperationFailure::ObjectUnavailable),
        )
            .into_response(),
        CallOutcome::Failed(failure) => failed(failure),
    }
}

pub(crate) async fn migrate<Platform: CentralPlatform>(
    State(app): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Query(query): Query<CopyPageQuery>,
) -> Response {
    use collaboration_central_admin_protocol::{
        CopyMigrationItem, CopyMigrationPage, CopyMigrationResult,
    };
    let owner = match OwnerIdentity::from_access(caller) {
        Ok(owner) => owner,
        Err(refusal) => return (StatusCode::UNAUTHORIZED, Json(refusal)).into_response(),
    };
    let requested = query.limit.unwrap_or(25);
    if !(1..=25).contains(&requested) {
        return (
            StatusCode::BAD_REQUEST,
            Json(OwnerOperationFailure::InvalidPageLimit),
        )
            .into_response();
    }
    let limit = match CopyPageLimit::try_from(requested) {
        Ok(limit) => limit,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(OwnerOperationFailure::InvalidPageLimit),
            )
                .into_response();
        }
    };
    let page = match app
        .call_from_owner::<FleetKind, _>(
            owner.clone(),
            (),
            ReadCopyDirectoryPage,
            CopyPageRequest {
                after: query.after,
                limit,
            },
        )
        .await
    {
        CallOutcome::Answered(page) => page,
        CallOutcome::Failed(failure) => return failed(failure),
    };
    let mut results = Vec::with_capacity(page.projects.len());
    for entry in page.projects {
        let database =
            match fleet_sync_protocol::DatabaseId::try_from(entry.project.as_str().to_owned()) {
                Ok(database) => database,
                Err(_) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(OwnerOperationFailure::ObjectUnavailable),
                    )
                        .into_response();
                }
            };
        let result = match app
            .call_from_owner::<CopyKind, _>(
                owner.clone(),
                database,
                CopyOperations { database },
                CopyOpsCall::Migrate,
            )
            .await
        {
            CallOutcome::Answered(CopyOpsAnswer::Migration(result)) => result,
            // A lost answer is an unavailable outcome, not evidence that the
            // transaction rolled back. Idempotent migration can be retried.
            CallOutcome::Failed(failure) => CopyMigrationResult::Failed {
                corpus: "fleet-sync-receiver".into(),
                version: None,
                reason: format!("migration outcome unavailable: {failure:?}"),
            },
            CallOutcome::Answered(_) => CopyMigrationResult::Failed {
                corpus: "fleet-sync-receiver".into(),
                version: None,
                reason: "Copy returned an invalid migration outcome".into(),
            },
        };
        results.push(CopyMigrationItem {
            project: entry.project,
            result,
        });
    }
    Json(CopyMigrationPage {
        results,
        next_after: page.next_after,
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleet_sync_protocol::CopyUnavailableReason;
    use serde_json::json;

    #[test]
    fn owner_http_preserves_full_and_other_shared_unavailable_reasons()
    -> Result<(), Box<dyn std::error::Error>> {
        for (reason, expected) in [
            (
                CopyUnavailableReason::StorageFull,
                json!({"kind":"centralCopyFull"}),
            ),
            (
                CopyUnavailableReason::MigrationPending { version: 3 },
                json!({"kind":"copyUnavailable","reason":{"kind":"migrationPending","value":{"version":3}}}),
            ),
            (
                CopyUnavailableReason::Damaged {
                    detail: "stored shape".into(),
                },
                json!({"kind":"copyUnavailable","reason":{"kind":"damaged","value":{"detail":"stored shape"}}}),
            ),
        ] {
            // The reference is PD3's public 503 body; input uses the real shared codec.
            let response = failed(fleet_sync_receiver::retryable_copy(reason));
            if response.status() != StatusCode::SERVICE_UNAVAILABLE {
                return Err("owner operation failure lost HTTP503".into());
            }
            let mut body = std::pin::pin!(axum::body::to_bytes(response.into_body(), 4096));
            let mut context = std::task::Context::from_waker(std::task::Waker::noop());
            let std::task::Poll::Ready(bytes) =
                std::future::Future::poll(body.as_mut(), &mut context)
            else {
                return Err("an in-memory owner response unexpectedly blocked".into());
            };
            let bytes = bytes?;
            if serde_json::from_slice::<serde_json::Value>(&bytes)? != expected {
                return Err("owner operation failure changed its typed wire body".into());
            }
        }
        Ok(())
    }
}
