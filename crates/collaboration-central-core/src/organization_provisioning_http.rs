//! HTTP policy for the private sign-in link; hosts mount it only on that link.
use crate::{
    identity_contract::{
        AUTHORIZATION_REVISIONS_ROUTE, AuthorizationRevision, ORGANIZATION_STATEMENT_DIGEST_ROUTE,
        PROVISION_ORGANIZATION_ROUTE, ProvisionOrganization, ReadOrganizationStatementDigest,
    },
    organization_authority::{OrganizationProvisioningFailure, OrganizationProvisioningPort},
};
use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Serialize;
use std::sync::Arc;

/// The host supplies the Fleet capability; this router opens no store.
pub fn organization_provisioning_router<Provisioner: OrganizationProvisioningPort>(
    provisioner: Arc<Provisioner>,
) -> Router {
    Router::new()
        .route(PROVISION_ORGANIZATION_ROUTE, post(provision::<Provisioner>))
        .route(
            ORGANIZATION_STATEMENT_DIGEST_ROUTE,
            post(statement_digest::<Provisioner>),
        )
        .route(
            AUTHORIZATION_REVISIONS_ROUTE,
            post(authorization_revision::<Provisioner>),
        )
        .with_state(provisioner)
}

async fn provision<Provisioner: OrganizationProvisioningPort>(
    State(provisioner): State<Arc<Provisioner>>,
    request: Result<Json<ProvisionOrganization>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = request else {
        return wrong_operation();
    };
    match provisioner.provision_organization(&request).await {
        Ok(answer) => (StatusCode::OK, Json(answer)).into_response(),
        Err(failure) => failure_status(failure).into_response(),
    }
}

async fn statement_digest<Provisioner: OrganizationProvisioningPort>(
    State(provisioner): State<Arc<Provisioner>>,
    request: Result<Json<ReadOrganizationStatementDigest>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = request else {
        return wrong_operation();
    };
    match provisioner.read_statement_digest(&request).await {
        Ok(answer) => (StatusCode::OK, Json(answer)).into_response(),
        Err(failure) => failure_status(failure).into_response(),
    }
}

async fn authorization_revision<Provisioner: OrganizationProvisioningPort>(
    State(provisioner): State<Arc<Provisioner>>,
    request: Result<Json<AuthorizationRevision>, JsonRejection>,
) -> Response {
    let Ok(Json(revision)) = request else {
        return wrong_operation();
    };
    match provisioner.deliver_authorization_revision(&revision).await {
        Ok(answer) => (StatusCode::OK, Json(answer)).into_response(),
        Err(failure) => failure_status(failure).into_response(),
    }
}

/// Error bodies are empty: only a 200 with the closed answer is a contract.
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

#[cfg(test)]
#[path = "organization_provisioning_http_test.rs"]
mod tests;
