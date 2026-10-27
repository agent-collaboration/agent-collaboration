//! The same Archived-board refusal and owning-row observations run on both runtimes.
#![cfg(feature = "conformance")]

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/clocked_server.rs"]
mod clocked_server;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/imported_claim_scenarios.rs"]
mod imported_claim_scenarios;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "fleet_storage/support.rs"]
mod native_fleet;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "mail_submit_http/registry_setup.rs"]
mod registry_setup;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
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
