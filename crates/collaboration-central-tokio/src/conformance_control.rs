//! Owner-authenticated controls stay outside the production core HTTP surface.

use crate::TokioCentralPlatform;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use collaboration_central_conformance::RuntimeControlRequest;
use collaboration_central_core::{AccessCaller, AccessKeyCache, AccessPolicy, CentralApplication};
use std::sync::Arc;

const ROUTE: &str = "/admin/v1/conformance/runtime";
struct Controls {
    application: CentralApplication<TokioCentralPlatform>,
    platform: TokioCentralPlatform,
}
pub(crate) fn mount(
    router: Router,
    platform: TokioCentralPlatform,
    policy: AccessPolicy,
) -> Result<Router, collaboration_central_core::AccessRefusal> {
    let application = CentralApplication::new(
        platform.clone(),
        policy,
        Arc::new(AccessKeyCache::default()),
    )?;
    Ok(router.merge(
        Router::new()
            .route(ROUTE, post(control))
            .route("/__conformance/admission", post(admission))
            .route(
                collaboration_central_core::FORWARD_STATE_PATH,
                post(forward_state),
            )
            .with_state(Arc::new(Controls {
                application,
                platform,
            })),
    ))
}

#[derive(serde::Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum AdmissionObservation {
    SessionConnections { service: message_board::ServiceId },
}

async fn admission(
    State(controls): State<Arc<Controls>>,
    headers: HeaderMap,
    Json(request): Json<AdmissionObservation>,
) -> Response {
    let assertion = headers
        .get("Cf-Access-Jwt-Assertion")
        .and_then(|value| value.to_str().ok());
    match controls
        .application
        .verify_access(assertion, "/admin/v1/conformance")
        .await
    {
        Ok(AccessCaller::Owner { .. }) => {}
        Ok(_) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(refusal) => return (StatusCode::UNAUTHORIZED, Json(refusal)).into_response(),
    }
    let AdmissionObservation::SessionConnections { service } = request;
    match controls
        .platform
        .conformance_session_connections(&service)
        .await
    {
        Ok(connections) => Json(connections).into_response(),
        Err(error) => (StatusCode::CONFLICT, Json(error.to_string())).into_response(),
    }
}
async fn control(
    State(controls): State<Arc<Controls>>,
    headers: HeaderMap,
    Json(request): Json<RuntimeControlRequest>,
) -> Response {
    let assertion = headers
        .get("Cf-Access-Jwt-Assertion")
        .and_then(|value| value.to_str().ok());
    match controls.application.verify_access(assertion, ROUTE).await {
        Ok(AccessCaller::Owner { .. }) => {}
        Ok(_) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(refusal) => return (StatusCode::UNAUTHORIZED, Json(refusal)).into_response(),
    }
    match controls.platform.runtime_control(request).await {
        Ok(observed) => Json(observed).into_response(),
        Err(error) => (StatusCode::CONFLICT, Json(error.to_string())).into_response(),
    }
}

async fn forward_state(
    State(controls): State<Arc<Controls>>,
    headers: HeaderMap,
    Json(request): Json<collaboration_central_core::ForwardStateRequest>,
) -> Response {
    let assertion = headers
        .get("Cf-Access-Jwt-Assertion")
        .and_then(|value| value.to_str().ok());
    match controls
        .application
        .verify_access(assertion, collaboration_central_core::FORWARD_STATE_PATH)
        .await
    {
        Ok(AccessCaller::Owner { .. }) => {}
        Ok(_) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(refusal) => return (StatusCode::UNAUTHORIZED, Json(refusal)).into_response(),
    }
    match controls
        .platform
        .conformance_forward_state(&request.service)
        .await
    {
        Ok(observed) => Json(observed).into_response(),
        Err(error) => (StatusCode::CONFLICT, Json(error.to_string())).into_response(),
    }
}
