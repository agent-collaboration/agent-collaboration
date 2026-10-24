//! The same Archived-board refusal and owning-row observations run on both runtimes.
#![cfg(feature = "conformance")]

#[allow(
    dead_code,
    reason = "The shared server also exposes clock controls used by other scenarios."
)]
#[path = "support/clocked_server.rs"]
mod clocked_server;
#[allow(
    dead_code,
    reason = "The shared module also serves the Open-board Worker scenario."
)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;
#[allow(
    dead_code,
    reason = "The shared module also serves the Open-board Worker scenario."
)]
#[path = "support/imported_claim_scenarios.rs"]
mod imported_claim_scenarios;
#[allow(
    dead_code,
    reason = "Only the existing canonical Fleet import arranger is used here."
)]
#[path = "fleet_storage/support.rs"]
mod native_fleet;
#[allow(
    dead_code,
    reason = "Only the existing public ImportTask/Copy seed arranger is used here."
)]
#[path = "mail_submit_http/registry_setup.rs"]
mod registry_setup;

#[path = "support/imported_board_fixture.rs"]
mod imported_board_fixture;

use clocked_server::configuration;
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer};
use collaboration_central_core::ConformanceBoardState;

#[tokio::test]
async fn archived_unretired_claim_refuses_without_writing_a_reservation() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let database = imported_board_fixture::arrange_imported_unheld_board(
        root.path(),
        ConformanceBoardState::Archived,
    )
    .await?;
    let jwks = JwksFixtureServer::start().await?;
    let server = clocked_server::ClockedServer::start(root.path(), &jwks.url).await?;
    let proof = imported_claim_scenarios::archived_claim_is_refused_without_reservation(
        server.base_url(),
        database,
    )
    .await;
    let stopped = server.shutdown().await;
    let keys_stopped = jwks.shutdown().await;
    proof?;
    stopped?;
    keys_stopped?;
    Ok(())
}
