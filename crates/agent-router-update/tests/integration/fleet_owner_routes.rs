//! Normal Fleet conflicts preserve exact decisions and the feed.
use crate::{central_fixture, update_support::UpdateResult as TestResult};
use collaboration_central_conformance::{
    ConformanceClient, JwksFixtureServer, run_composed_fleet_conflict,
};

#[tokio::test]
async fn normal_fleet_conflict_preserves_exact_decision_records_and_feed() -> TestResult {
    // Reuse the current real-session corpus replacing the deleted LocalServer
    // wrapper; no second decision model or fake Fleet snapshot is introduced.
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let server = central_fixture::start_server(root.path(), &jwks).await?;
    let client = ConformanceClient::new(server.base_url())
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let observed = run_composed_fleet_conflict(&client).await;
    let stopped = server.shutdown().await;
    let keys_stopped = jwks.shutdown().await;
    stopped?;
    keys_stopped.map_err(|error| error as Box<dyn std::error::Error>)?;
    observed.map_err(|error| error as Box<dyn std::error::Error>)
}
