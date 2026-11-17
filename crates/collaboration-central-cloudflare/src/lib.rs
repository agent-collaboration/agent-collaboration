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
mod workspace_store_object;
#[cfg(feature = "test-support")]
pub use driver_scenarios::DriverScenarioObject;
#[cfg(feature = "test-support")]
pub use storage_cases::StorageCaseObject;
pub use workspace_central_object::CentralObject;
pub use workspace_store_object::WorkspaceStoreObject;

/// The test-support bundle's fetch entry: it answers only the storage and workspace
/// case routes. The public Worker has no fetch entry; the private link Worker
/// (`--features link`) mounts its own.
#[cfg(all(feature = "test-support", not(feature = "link")))]
#[worker::event(fetch)]
pub async fn main(
    request: worker::Request,
    env: worker::Env,
    _context: worker::Context,
) -> worker::Result<worker::Response> {
    match storage_cases::route(request, &env).await? {
        Ok(response) => Ok(response),
        Err(_) => worker::Response::error("no such route", 404),
    }
}
