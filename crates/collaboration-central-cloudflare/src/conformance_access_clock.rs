//! Isolate-owned Access time exists only in the local dev conformance artifact.
use crate::access_verification::{self, InvocationLabels, VerificationLayer};
use collaboration_central_core::AccessCaller;
use runtime_primitives::SyncTime;
use worker::{Env, Request, Response, Result};

pub(crate) async fn set_time(mut request: Request, env: Env) -> Result<Response> {
    let labels = InvocationLabels::for_request(&request)?;
    let assertion = request.headers().get("Cf-Access-Jwt-Assertion")?;
    match access_verification::verify(
        &env,
        assertion.as_deref(),
        collaboration_central_conformance::LOCAL_ACCESS_CLOCK_PATH,
        &labels,
        VerificationLayer::Edge,
    )
    .await
    {
        Ok(AccessCaller::Owner { .. }) => {}
        _ => return Response::error("owner required", 401),
    }
    let now: chrono::DateTime<chrono::Utc> = request.json().await?;
    crate::platform::CloudflareClock::set_local_time(SyncTime::from_unix_millis(
        now.timestamp_millis(),
    ));
    Response::from_json(&now)
}
