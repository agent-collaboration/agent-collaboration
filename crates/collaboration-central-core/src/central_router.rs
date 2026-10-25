//! Shared HTTP policy. Adapters transport calls and perform the physical upgrade.

use crate::{
    AccessCaller, AccessRefusal, CallOutcome, CentralApplication, CentralPlatform, EnrollMachine,
    RegistryCall, RegistryKind, RegistryResult, UpgradeFailure,
};
use axum::{
    Extension, Json, Router,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use fleet_sync_protocol::{AuthRefusal, ConnectionId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub fn central_router<Platform: CentralPlatform>(
    application: CentralApplication<Platform>,
) -> Router {
    let application = Arc::new(application);
    let router = Router::new()
        .route(
            "/admin/v1/copies/{id}/health",
            get(crate::owner_http_routes::health::<Platform>),
        )
        .route(
            "/admin/v1/copies",
            get(crate::owner_http_routes::copies::<Platform>),
        )
        .route(
            "/admin/v1/copies/{id}/repair",
            post(crate::owner_http_routes::repair::<Platform>),
        )
        .route(
            "/admin/v1/copies/migrate",
            post(crate::owner_http_routes::migrate::<Platform>),
        )
        .route(
            "/admin/v1/import/fleet",
            post(crate::admin_import_routes::fleet::<Platform>),
        )
        .route(
            "/admin/v1/import/projects/{id}",
            post(crate::admin_import_routes::project::<Platform>),
        )
        .route("/admin/v1/machines/enroll", post(enroll::<Platform>))
        .route(
            "/admin/v1/projects/{id}/move",
            post(crate::owner_registry_http::move_project::<Platform>),
        )
        .route(
            "/admin/v1/projects/{id}/takeover/preview",
            post(crate::owner_registry_http::preview::<Platform>),
        )
        .route(
            "/admin/v1/projects/{id}/takeover",
            post(crate::owner_registry_http::takeover::<Platform>),
        )
        .route("/admin/v1/machines/revoke", post(revoke::<Platform>))
        .route(
            "/admin/v1/projects/{id}/retire",
            post(crate::owner_registry_http::retire::<Platform>),
        )
        .route("/v1/session", get(open_session::<Platform>));
    #[cfg(feature = "conformance")]
    let router = router
        .route(
            "/admin/v1/conformance/fleet-receipt",
            post(crate::conformance_fleet::control::<Platform>),
        )
        .route(
            "/admin/v1/conformance/revocation",
            post(crate::machine_enrollment::conformance_revocation::control::<Platform>),
        )
        .route(
            "/admin/v1/conformance/access",
            post(crate::conformance_access::verify::<Platform>),
        )
        .route(
            "/admin/v1/conformance/holder-call",
            post(crate::conformance_holder::control::<Platform>),
        )
        .route(
            "/admin/v1/conformance/holder-status",
            post(crate::conformance_holder::status::<Platform>),
        )
        .route(
            "/admin/v1/conformance/registry-dispatches",
            get(crate::conformance_holder::dispatches::<Platform>),
        )
        .route(
            "/admin/v1/conformance/copy-call",
            post(crate::conformance_copy::control::<Platform>),
        )
        .route(
            "/admin/v1/conformance/copy-status",
            post(crate::conformance_copy::status::<Platform>),
        )
        .route(
            "/admin/v1/conformance/copy-wait",
            post(crate::conformance_copy::wait_control::<Platform>),
        )
        .route(
            "/admin/v1/conformance/drive-tokens",
            get(crate::conformance_copy::drive_tokens::<Platform>),
        );
    router
        .fallback(|| async { (StatusCode::UNAUTHORIZED, Json(AccessRefusal::RouteRefused)) })
        .layer(middleware::from_fn_with_state(
            application.clone(),
            access_gate::<Platform>,
        ))
        .with_state(application)
}

async fn access_gate<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    mut request: Request,
    next: Next,
) -> Response {
    let assertion = request
        .headers()
        .get("Cf-Access-Jwt-Assertion")
        .and_then(|value| value.to_str().ok());
    match application
        .verify_access(assertion, request.uri().path())
        .await
    {
        Ok(caller) => {
            request.extensions_mut().insert(caller);
            next.run(request).await
        }
        Err(refusal) => (StatusCode::UNAUTHORIZED, Json(refusal)).into_response(),
    }
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum EnrollmentBody {
    EnrollMachine(EnrollMachine),
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum RevocationBody {
    RevokeMachine {
        installation: message_board::ServiceId,
    },
}

async fn revoke<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Json(RevocationBody::RevokeMachine { installation }): Json<RevocationBody>,
) -> Response {
    match application
        .call_from_edge::<RegistryKind>(caller, (), RegistryCall::RevokeEnrollment { installation })
        .await
    {
        CallOutcome::Answered(RegistryResult::EnrollmentRevoked(result)) => {
            Json(result).into_response()
        }
        CallOutcome::Answered(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(EdgeFailure::InvalidObjectReply),
        )
            .into_response(),
        CallOutcome::Failed(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(EdgeFailure::ObjectUnavailable),
        )
            .into_response(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum EdgeFailure {
    ObjectUnavailable,
    InvalidObjectReply,
    InvalidConnectionIdentity,
}

async fn enroll<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Json(EnrollmentBody::EnrollMachine(request)): Json<EnrollmentBody>,
) -> Response {
    match application
        .call_from_edge::<RegistryKind>(caller, (), RegistryCall::EnrollMachine(request))
        .await
    {
        CallOutcome::Answered(RegistryResult::Enrollment(result)) => Json(result).into_response(),
        CallOutcome::Answered(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(EdgeFailure::InvalidObjectReply),
        )
            .into_response(),
        CallOutcome::Failed(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(EdgeFailure::ObjectUnavailable),
        )
            .into_response(),
    }
}

fn bearer_hash(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty())
        .map(|token| format!("{:x}", Sha256::digest(token.as_bytes())))
}

async fn open_session<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    request: Request,
) -> Response {
    let Some(credential_hash) = bearer_hash(request.headers()) else {
        return credential_refusal(AuthRefusal::Unknown);
    };
    let admitted = match application
        .call_from_edge::<RegistryKind>(
            caller,
            (),
            RegistryCall::AuthenticateEnrollment { credential_hash },
        )
        .await
    {
        CallOutcome::Answered(RegistryResult::EnrollmentAuthenticated(Ok(admitted))) => admitted,
        CallOutcome::Answered(RegistryResult::EnrollmentAuthenticated(Err(refusal))) => {
            return credential_refusal(refusal);
        }
        CallOutcome::Answered(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(EdgeFailure::InvalidObjectReply),
            )
                .into_response();
        }
        CallOutcome::Failed(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(EdgeFailure::ObjectUnavailable),
            )
                .into_response();
        }
    };
    let connection = match ConnectionId::new(application.platform.ids().new_id()) {
        Ok(connection) => connection,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(EdgeFailure::InvalidConnectionIdentity),
            )
                .into_response();
        }
    };
    let binding = admitted.binding(connection);
    let origin = admitted.origin().clone();
    let (parts, _body) = request.into_parts();
    match application
        .platform
        .upgrade_session(parts, binding, origin)
        .await
    {
        Ok(response) => response,
        Err(failure @ UpgradeFailure::NotAnUpgrade) => {
            (StatusCode::BAD_REQUEST, Json(failure)).into_response()
        }
        Err(failure @ UpgradeFailure::Refused) => {
            (StatusCode::SERVICE_UNAVAILABLE, Json(failure)).into_response()
        }
    }
}

fn credential_refusal(refusal: AuthRefusal) -> Response {
    let status = if refusal == AuthRefusal::WrongKind {
        StatusCode::FORBIDDEN
    } else {
        StatusCode::UNAUTHORIZED
    };
    let encoded = match serde_json::to_string(&refusal) {
        Ok(encoded) => encoded,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    (
        status,
        [("fleet-sync-auth-refusal", encoded)],
        Json(refusal),
    )
        .into_response()
}
