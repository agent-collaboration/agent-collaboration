//! The original archive report assertions run through the composed native server.
#![cfg(feature = "conformance")]

#[path = "support/clocked_server.rs"]
mod clocked_server;
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, run_archive_report_with_clock,
};

#[tokio::test]
async fn archive_reports_preserve_first_report_and_exact_no_ledger_finalization()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = clocked_server::ClockedServer::start(root.path(), &jwks.url).await?;
    let client = ConformanceClient::new(server.base_url())?;
    let proof = run_archive_report_with_clock(&client, |at| {
        server.clock.set_time(at);
        std::future::ready(Ok(()))
    })
    .await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}
