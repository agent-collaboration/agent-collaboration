//! The central-link Worker's only entry: no public route reaches it, and it
//! forwards each private sign-in request to its organization's Fleet object.
use crate::{access_verification, http_body, platform::CloudflareCentralPlatform};
use worker::{Context, Env, Request, Response, Result, event};

#[event(fetch)]
pub async fn main(request: Request, env: Env, _context: Context) -> Result<Response> {
    let labels = access_verification::InvocationLabels::for_request(&request, &env)?;
    let request: worker::HttpRequest = request.try_into()?;
    if !request
        .uri()
        .path()
        .starts_with(collaboration_central_core::PRIVATE_LINK_PREFIX)
    {
        let mut response = Response::empty()?.with_status(404);
        response
            .headers_mut()
            .set(crate::ENTRY_HEADER, LINK_ENTRY)?;
        return Ok(response);
    }
    let (parts, body) = request.into_parts();
    let body = match http_body::complete(body).await? {
        http_body::BodyCompletion::Complete(body) => body,
        http_body::BodyCompletion::OverLimit => return http_body::over_limit(),
    };
    let request = http::Request::from_parts(parts, body);
    let locations = crate::cloudflare_transport::locations(&env)?;
    // The link verifies no Access assertion; its Fleet call carries the
    // sign-in link authority, and the Fleet object checks the stamp itself.
    let platform = CloudflareCentralPlatform::new(
        String::new(),
        labels,
        access_verification::VerificationLayer::Edge,
        env.clone(),
        locations.legacy_organization().clone(),
    );
    #[cfg(feature = "conformance")]
    let platform = platform.with_capabilities(
        crate::conformance_capabilities::ConformanceCapabilities::from_env(&env)?,
    );
    use tower_service::Service;
    let mut response = collaboration_central_core::organization_link_router(platform)
        .call(request)
        .await?;
    // Names the artifact: the bundle guard refuses this one as public central.
    response.headers_mut().insert(
        crate::ENTRY_HEADER,
        http::HeaderValue::from_static(LINK_ENTRY),
    );
    response.try_into()
}

const LINK_ENTRY: &str = "collaboration-central-link-entry";
