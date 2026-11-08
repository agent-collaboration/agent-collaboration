//! Cloudflare capabilities for the shared asynchronous application core.
#![forbid(unsafe_code)]
#![cfg_attr(
    feature = "link",
    expect(
        dead_code,
        reason = "the central-link build mounts only the private routes; the public entry stays in the shared crate"
    )
)]
mod access_crypto;
mod access_verification;
mod binding_attachment;
#[cfg(feature = "conformance")]
mod composed_control;
#[cfg(feature = "conformance")]
mod conformance_admission;
#[cfg(feature = "conformance")]
mod conformance_bootstrap;
#[cfg(feature = "conformance")]
mod conformance_capabilities;
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
mod reply_deadline;
mod shared_object_store;
mod sign_in_trust;
mod storage_measurement;
pub use object_shell::{FleetObject, ProjectCopyObject};
pub use shared_object_store::CloudflareStorageHost;
#[cfg(not(feature = "link"))]
use worker::{Context, Env, Request, Response, Result, event};
#[cfg(feature = "link")]
mod link_entry;
#[cfg(not(feature = "link"))]
#[event(fetch)]
pub async fn main(request: Request, env: Env, _context: Context) -> Result<Response> {
    let labels = access_verification::InvocationLabels::for_request(&request, &env)?;
    let request: worker::HttpRequest = request.try_into()?;
    let (parts, body) = request.into_parts();
    let body = match http_body::complete(body).await? {
        http_body::BodyCompletion::Complete(body) => body,
        http_body::BodyCompletion::OverLimit => return http_body::over_limit(),
    };
    // Retain headers, Cf, abort signal and upgrade parts while replacing only
    // the consumed network body with its bounded, completed bytes.
    let request = http::Request::from_parts(parts, body);
    #[cfg(feature = "conformance")]
    if request.uri().path() == conformance_bootstrap::CONTROL_PATH {
        return conformance_bootstrap::forward(&env, request.try_into()?).await;
    }
    #[cfg(feature = "conformance")]
    if request.uri().path() == composed_control::CONTROL_PATH {
        return composed_control::fetch(request.try_into()?, env).await;
    }
    #[cfg(feature = "conformance")]
    if request.uri().path() == collaboration_central_core::FLEET_SCHEMA_PATH {
        return conformance_admission::fleet_schema(request.try_into()?, env).await;
    }
    #[cfg(feature = "conformance")]
    if request.uri().path() == collaboration_central_core::ORGANIZATION_RECORDS_PATH {
        return conformance_admission::organization_records(request.try_into()?, env).await;
    }
    #[cfg(feature = "conformance")]
    if request.uri().path() == collaboration_central_core::FLEET_MAIL_CUSTODY_PATH {
        return conformance_admission::fleet_mail_custody(request.try_into()?, env).await;
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
    let mut response = collaboration_central_core::central_router(application)
        .call(request)
        .await?;
    // Names the artifact: the bundle guard refuses this one as central-link.
    response.headers_mut().insert(
        ENTRY_HEADER,
        http::HeaderValue::from_static("collaboration-central-public-entry"),
    );
    response.try_into()
}

/// Every response names which of the crate's two Worker entries served it.
pub(crate) const ENTRY_HEADER: &str = "x-collaboration-central-entry";
