//! HTTP policy for provisioning; hosts mount it only on the private sign-in link.
use crate::{
    identity_contract::{PROVISION_ORGANIZATION_ROUTE, ProvisionOrganization},
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

/// The host supplies the Fleet transaction capability; this router opens no store.
pub fn organization_provisioning_router<Provisioner: OrganizationProvisioningPort>(
    provisioner: Arc<Provisioner>,
) -> Router {
    Router::new()
        .route(PROVISION_ORGANIZATION_ROUTE, post(provision::<Provisioner>))
        .with_state(provisioner)
}

async fn provision<Provisioner: OrganizationProvisioningPort>(
    State(provisioner): State<Arc<Provisioner>>,
    request: Result<Json<ProvisionOrganization>, JsonRejection>,
) -> Response {
    let Ok(Json(request)) = request else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ProvisioningRequestRefusal::WrongOperation),
        )
            .into_response();
    };
    match provisioner.provision_organization(&request).await {
        Ok(answer) => (StatusCode::OK, Json(answer)).into_response(),
        Err(OrganizationProvisioningFailure::OrganizationMismatch) => {
            StatusCode::FORBIDDEN.into_response()
        }
        Err(
            OrganizationProvisioningFailure::Unavailable
            | OrganizationProvisioningFailure::OutcomeUnknown,
        ) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum ProvisioningRequestRefusal {
    WrongOperation,
}

#[cfg(test)]
#[path = "organization_provisioning_http_test.rs"]
mod tests;
