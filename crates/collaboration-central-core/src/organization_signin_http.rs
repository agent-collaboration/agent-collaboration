//! HTTP policy shared by both hosts' private sign-in links.
use crate::{
    identity_contract::{
        AUTHORIZATION_REVISIONS_ROUTE, AuthorizationRevision, ORGANIZATION_STATEMENT_DIGEST_ROUTE,
        PROVISION_ORGANIZATION_ROUTE, ProvisionOrganization, ReadOrganizationStatementDigest,
    },
    organization_admin_contract::OrganizationAdminAnswer,
    organization_authority::OrganizationProvisioningFailure,
    organization_signin_port::OrganizationSignInPort,
    person_request::{AdminRoute, PersonRequestInput},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use fleet_identity_protocol::OrganizationId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Mount verified organization setup and reads only on the private link.
pub fn organization_signin_router<Port: OrganizationSignInPort>(port: Arc<Port>) -> Router {
    let mut router = Router::new()
        .route(PROVISION_ORGANIZATION_ROUTE, post(provision::<Port>))
        .route(
            ORGANIZATION_STATEMENT_DIGEST_ROUTE,
            post(statement_digest::<Port>),
        )
        .route(
            AUTHORIZATION_REVISIONS_ROUTE,
            post(authorization_revision::<Port>),
        )
        .with_state(port.clone());
    for route in [
        AdminRoute::CreateWorkspace,
        AdminRoute::WorkspaceLookup,
        AdminRoute::ReadChangeRecords,
    ] {
        let port = port.clone();
        router = router.route(
            route.path(),
            post(move |headers: HeaderMap, body: Bytes| {
                forward_admin(port.clone(), route, headers, body)
            }),
        );
    }
    router
}

async fn provision<Port: OrganizationSignInPort>(
    State(port): State<Arc<Port>>,
    request: Result<Json<ProvisionOrganization>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = request else {
        return wrong_operation();
    };
    match port.provision(&request).await {
        Ok(answer) => (StatusCode::OK, Json(answer)).into_response(),
        Err(failure) => failure_status(failure).into_response(),
    }
}

async fn statement_digest<Port: OrganizationSignInPort>(
    State(port): State<Arc<Port>>,
    request: Result<Json<ReadOrganizationStatementDigest>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = request else {
        return wrong_operation();
    };
    match port.digest(&request).await {
        Ok(answer) => (StatusCode::OK, Json(answer)).into_response(),
        Err(failure) => failure_status(failure).into_response(),
    }
}

async fn authorization_revision<Port: OrganizationSignInPort>(
    State(port): State<Arc<Port>>,
    request: Result<Json<AuthorizationRevision>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = request else {
        return wrong_operation();
    };
    match port.revision(&request).await {
        Ok(answer) => (StatusCode::OK, Json(answer)).into_response(),
        Err(failure) => failure_status(failure).into_response(),
    }
}

/// Only for addressing; the scoped runtime re-parses and verifies the whole body.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Addressed {
    organization_id: OrganizationId,
}

async fn forward_admin<Port: OrganizationSignInPort>(
    port: Arc<Port>,
    route: AdminRoute,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let (Some(authorization), Some(proof)) = (header("authorization"), header("dpop")) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Ok(body) = String::from_utf8(body.to_vec()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Ok(Addressed { organization_id }) = serde_json::from_str::<Addressed>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let input = PersonRequestInput {
        route,
        method: "POST".into(),
        authorization,
        proof,
        body,
    };
    match port.admin(organization_id, input).await {
        Ok(OrganizationAdminAnswer::Unauthenticated) => StatusCode::UNAUTHORIZED.into_response(),
        Ok(OrganizationAdminAnswer::Answered(result)) => {
            (StatusCode::OK, Json(result)).into_response()
        }
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

/// Failure bodies are empty; only a 200 publishes a closed result.
fn failure_status(failure: OrganizationProvisioningFailure) -> StatusCode {
    match failure {
        OrganizationProvisioningFailure::OrganizationMismatch => StatusCode::FORBIDDEN,
        OrganizationProvisioningFailure::InvalidStatement => StatusCode::UNPROCESSABLE_ENTITY,
        OrganizationProvisioningFailure::StaleStatement => StatusCode::CONFLICT,
        OrganizationProvisioningFailure::OrganizationFloorMissing => {
            StatusCode::PRECONDITION_FAILED
        }
        OrganizationProvisioningFailure::Unprovisioned => StatusCode::NOT_FOUND,
        OrganizationProvisioningFailure::Unavailable
        | OrganizationProvisioningFailure::OutcomeUnknown => StatusCode::SERVICE_UNAVAILABLE,
    }
}

fn wrong_operation() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ProvisioningRequestRefusal::WrongOperation),
    )
        .into_response()
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum ProvisioningRequestRefusal {
    WrongOperation,
}
