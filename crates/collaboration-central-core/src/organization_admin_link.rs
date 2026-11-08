//! The private link's three admin routes. The link forwards the raw token,
//! proof and body to the named organization's Fleet object, which verifies
//! and applies them; the link itself decides nothing.
use crate::{
    CallerContext, CentralPlatform, EdgeKind, FleetAddress, FleetKind,
    organization_admin_calls::{FleetOrganizationAdmin, OrganizationAdminAnswer},
    person_request::{AdminRoute, PersonRequestInput},
};
use axum::{
    Router,
    body::Bytes,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use durable_steps::{CallFailure, CallId, OperationId, StepName};
use fleet_identity_protocol::OrganizationId;
use serde::Deserialize;
use std::sync::Arc;

pub(crate) fn admin_link_routes<Platform: CentralPlatform>(platform: Platform) -> Router {
    let platform = Arc::new(platform);
    let mut router = Router::new();
    for route in [
        AdminRoute::CreateWorkspace,
        AdminRoute::WorkspaceLookup,
        AdminRoute::ReadChangeRecords,
    ] {
        let platform = platform.clone();
        router = router.route(
            route.path(),
            post(move |headers: HeaderMap, body: Bytes| {
                forward(platform.clone(), route, headers, body)
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

async fn forward<Platform: CentralPlatform>(
    platform: Arc<Platform>,
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
    match call(platform.as_ref(), organization_id, input).await {
        Ok(OrganizationAdminAnswer::Unauthenticated) => StatusCode::UNAUTHORIZED.into_response(),
        Ok(answer) => (StatusCode::OK, axum::Json(answer)).into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn call<Platform: CentralPlatform>(
    platform: &Platform,
    organization: OrganizationId,
    input: PersonRequestInput,
) -> Result<OrganizationAdminAnswer, CallFailure> {
    let caller = CallerContext::<EdgeKind>::for_sign_in_link(organization.clone());
    let identity = CallId {
        operation: OperationId::new(platform.ids().new_id())
            .map_err(|_| CallFailure::AnswerLost)?,
        step: StepName::new("organization_admin").map_err(|_| CallFailure::AnswerLost)?,
    };
    let call = crate::object_call_envelope::encode_shared_call::<
        EdgeKind,
        FleetKind,
        FleetOrganizationAdmin,
    >(
        &caller,
        FleetAddress::new(organization),
        FleetOrganizationAdmin,
        input,
        identity,
    )?;
    let reply = platform.transport_object(call).await?;
    serde_json::from_slice(&reply).map_err(|_| CallFailure::AnswerLost)
}
