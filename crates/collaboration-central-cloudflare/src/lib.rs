//! Cloudflare capabilities for the shared asynchronous application core.
#![forbid(unsafe_code)]
mod access_crypto;
mod access_verification;
mod binding_attachment;
mod client_assertion_signer;
#[cfg(feature = "conformance")]
mod composed_control;
#[cfg(feature = "conformance")]
mod conformance_admission;
#[cfg(feature = "conformance")]
mod conformance_bootstrap;
#[cfg(feature = "conformance")]
mod conformance_client_authentication;
#[cfg(feature = "conformance")]
mod conformance_core_store;
#[cfg(feature = "conformance")]
mod conformance_issuer_signature;
#[cfg(feature = "conformance")]
mod conformance_person_proof;
mod issuer_crypto;
pub use client_assertion_signer::CloudflareEd25519ClientSigner;
mod service_form_transport;
pub use service_form_transport::CloudflareServiceFormTransport;
mod issuer_key_source;
#[cfg(feature = "conformance")]
pub use conformance_bootstrap::BootstrapObject;
pub use issuer_key_source::CloudflareIssuerKeySource;
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
    #[cfg(feature = "conformance")]
    if request.path() == collaboration_central_core::CLIENT_AUTH_PROBE_PATH {
        return conformance_client_authentication::fetch(request, env).await;
    }
    #[cfg(feature = "conformance")]
    if request.path() == collaboration_central_core::PERSON_PROOF_PROBE_PATH {
        return conformance_person_proof::fetch(request, env).await;
    }
    #[cfg(feature = "conformance")]
    if request.path() == collaboration_central_core::ISSUER_SIGNATURE_PROBE_PATH {
        return conformance_issuer_signature::fetch(request, env).await;
    }
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
