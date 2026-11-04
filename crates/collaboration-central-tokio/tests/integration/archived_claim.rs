//! The same Archived-board refusal and owning-row observations run on both runtimes.
#![cfg(feature = "conformance")]

use crate::clocked_server;

use crate::imported_claim_scenarios;

use crate::imported_board_fixture;

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
