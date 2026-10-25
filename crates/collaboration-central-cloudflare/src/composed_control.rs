//! Owner controls use the composed object's physical namespace, never PROBE_*.

use crate::{access_verification, cloudflare_transport};
use collaboration_central_conformance::{RuntimeControlRequest, RuntimeFaultObservation};
use collaboration_central_core::{AccessCaller, ObjectAddress};
use worker::*;

pub(crate) const CONTROL_PATH: &str = "/admin/v1/conformance/runtime";
pub(crate) const OBJECT_PATH: &str = "/test/runtime-control";
pub(crate) const CLOCK_KEY: &str = "_cf_composed_clock";

pub(crate) async fn fetch(mut request: Request, env: Env) -> Result<Response> {
    let labels = access_verification::InvocationLabels::for_request(&request)?;
    let assertion = request.headers().get("Cf-Access-Jwt-Assertion")?;
    match access_verification::verify(
        &env,
        assertion.as_deref(),
        CONTROL_PATH,
        &labels,
        access_verification::VerificationLayer::Edge,
    )
    .await
    {
        Ok(AccessCaller::Owner { .. }) => {}
        _ => return Response::error("owner required", 401),
    }
    let request: RuntimeControlRequest = request.json().await?;
    if let Err(error) = request.validate() {
        return observation(Err(error));
    }
    forward(
        &env,
        &request.address,
        OBJECT_PATH,
        Some(serde_json::to_string(&request.command)?),
    )
    .await
}

pub(crate) async fn forward(
    env: &Env,
    address: &ObjectAddress,
    path: &str,
    body: Option<String>,
) -> Result<Response> {
    let headers = Headers::new();
    cloudflare_transport::address_header(&headers, address)?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(body.map(Into::into));
    cloudflare_transport::stub(env, address)?
        .fetch_with_request(Request::new_with_init(
            &format!("https://central.internal{path}"),
            &init,
        )?)
        .await
}

pub(crate) fn observation(
    observed: std::result::Result<
        RuntimeFaultObservation,
        collaboration_central_conformance::RuntimeControlFailure,
    >,
) -> Result<Response> {
    match observed {
        Ok(observed) => Response::from_json(&observed),
        Err(error) => Ok(Response::from_json(&error)?.with_status(409)),
    }
}
