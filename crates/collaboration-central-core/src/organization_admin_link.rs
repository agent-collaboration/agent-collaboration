//! The private link's three admin routes. The link forwards the raw token,
//! proof and body to the named organization's Fleet object, which verifies
//! and applies them; the link itself decides nothing.
use crate::{
    organization_admin_calls::OrganizationAdminAnswer,
    person_request::{AdminRoute, PersonRequestInput},
};
use axum::{
    Router,
    body::Bytes,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use durable_steps::CallFailure;
use fleet_identity_protocol::OrganizationId;
use serde::Deserialize;
use std::{future::Future, sync::Arc};

/// Carries one admin request to the named organization's Fleet object.
pub(crate) trait OrganizationAdminPort: Send + Sync + 'static {
    fn admin(
        &self,
        organization: OrganizationId,
        input: PersonRequestInput,
    ) -> impl Future<Output = Result<OrganizationAdminAnswer, CallFailure>> + Send;
}

pub(crate) fn admin_link_routes<Port: OrganizationAdminPort>(port: Port) -> Router {
    let port = Arc::new(port);
    let mut router = Router::new();
    for route in [
        AdminRoute::CreateWorkspace,
        AdminRoute::WorkspaceLookup,
        AdminRoute::ReadChangeRecords,
    ] {
        let port = port.clone();
        router = router.route(
            route.path(),
            post(move |headers: HeaderMap, body: Bytes| {
                forward(port.clone(), route, headers, body)
            }),
        );
    }
    router
}

/// Only to address the Fleet object; the Fleet re-parses the whole body.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Addressed {
    organization_id: OrganizationId,
}

async fn forward<Port: OrganizationAdminPort>(
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
            (StatusCode::OK, axum::Json(result)).into_response()
        }
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
