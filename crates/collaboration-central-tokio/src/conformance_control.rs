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
            .with_state(Arc::new(Controls {
                application,
                platform,
            })),
    ))
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
