//! Cloudflare capabilities for the shared asynchronous application core.
#![forbid(unsafe_code)]
#![cfg_attr(
    feature = "link",
    expect(
        dead_code,
        reason = "the private link build shares object exports but mounts only its private fetch entry"
    )
)]
mod cloudflare_transport;
#[cfg(feature = "test-support")]
mod driver_scenarios;
mod http_body;
mod object_location;
mod object_migrations;
mod organization_signin_link;
mod sign_in_crypto;
mod sign_in_trust;
mod signin_observation;
mod statement_store;
#[cfg(feature = "test-support")]
mod storage_cases;
mod store_wire;
mod worker_clock;
mod workspace_central_object;
mod workspace_link_entry;
#[cfg(feature = "test-support")]
mod workspace_rpc_fixture;
mod workspace_store_object;
#[cfg(feature = "test-support")]
pub use driver_scenarios::DriverScenarioObject;
#[cfg(feature = "test-support")]
pub use storage_cases::StorageCaseObject;
pub use workspace_central_object::CentralObject;
pub use workspace_store_object::WorkspaceStoreObject;

/// Carrier-facing ingress for the machine link.
#[cfg(not(feature = "link"))]
#[worker::event(fetch)]
pub async fn main(
    request: worker::Request,
    env: worker::Env,
    _context: worker::Context,
) -> worker::Result<worker::Response> {
    #[cfg(feature = "test-support")]
    let request = match storage_cases::route(request, &env).await? {
        Ok(response) => return Ok(response),
        Err(request) => request,
    };
    if workspace_link_entry::organization(&request.path()).is_some() {
        return workspace_link_entry::forward(request, &env).await;
    }
    worker::Response::error("no such route", 404)
}
