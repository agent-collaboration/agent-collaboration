//! Cloudflare capabilities for the shared asynchronous application core.
#![forbid(unsafe_code)]
#[cfg(all(feature = "local-access-clock", not(debug_assertions)))]
compile_error!("local-access-clock is forbidden in release artifacts");
mod access_crypto;
mod access_verification;
mod binding_attachment;
#[cfg(feature = "conformance")]
mod composed_control;
#[cfg(feature = "local-access-clock")]
mod conformance_access_clock;
#[cfg(feature = "conformance")]
mod conformance_admission;
#[cfg(feature = "conformance")]
mod conformance_bootstrap;
#[cfg(feature = "conformance")]
mod conformance_core_store;
#[cfg(feature = "conformance")]
pub use conformance_bootstrap::BootstrapObject;
mod cloudflare_transport;
#[cfg(feature = "conformance")]
mod conformance_scope_faults;
mod http_body;
mod object_location;
mod object_shell;
mod platform;
mod shared_object_store;
mod storage_measurement;
pub use object_shell::{
    ConnectionObject, FleetObject, LeaseRegistryObject, MailboxObject, ProjectCopyObject,
};
pub use shared_object_store::CloudflareStorageHost;
use worker::{Context, Env, Request, Response, Result, event};
#[event(fetch)]
pub async fn main(request: Request, env: Env, _context: Context) -> Result<Response> {
    let labels = access_verification::InvocationLabels::for_request(&request)?;
    let request: worker::HttpRequest = request.try_into()?;
    let (parts, body) = request.into_parts();
    let body = match http_body::complete(body).await? {
        http_body::BodyCompletion::Complete(body) => body,
        http_body::BodyCompletion::OverLimit => return http_body::over_limit(),
    };
    // Retain headers, Cf, abort signal and upgrade parts while replacing only
    // the consumed network body with its bounded, completed bytes.
    let request = http::Request::from_parts(parts, body);
    #[cfg(feature = "local-access-clock")]
    if request.uri().path() == collaboration_central_conformance::LOCAL_ACCESS_CLOCK_PATH {
        return conformance_access_clock::set_time(request.try_into()?, env).await;
    }
    #[cfg(feature = "conformance")]
    if request.uri().path() == conformance_bootstrap::CONTROL_PATH {
        return conformance_bootstrap::forward(&env, request.try_into()?).await;
    }
    #[cfg(feature = "conformance")]
    if request.uri().path() == composed_control::CONTROL_PATH {
        return composed_control::fetch(request.try_into()?, env).await;
    }
    #[cfg(feature = "conformance")]
    if request.uri().path() == collaboration_central_core::FORWARD_STATE_PATH {
        return conformance_admission::forward_state(request.try_into()?, env).await;
    }
    #[cfg(feature = "conformance")]
    if request.uri().path() == "/__conformance/admission" {
        return conformance_admission::fetch(request.try_into()?, env).await;
    }
    use tower_service::Service;
    let application = access_verification::application(
        &env,
        labels,
        access_verification::VerificationLayer::Edge,
    )
    .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let response = collaboration_central_core::central_router(application)
        .call(request)
        .await?;
    response.try_into()
}
