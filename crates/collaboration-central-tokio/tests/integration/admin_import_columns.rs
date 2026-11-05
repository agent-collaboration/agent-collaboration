//! Normal owner routes publish a real supplied snapshot and retain its continuation across reopen.
#![cfg(feature = "conformance")]
use crate::clocked_server::ClockedServer;
use collaboration_central_conformance::{
    AdminImportScenario, ConformanceResult, JwksFixtureServer,
};

#[tokio::test]
async fn normal_admin_import_keeps_original_header_and_completion_after_whole_server_reopen()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let result = async {
        let server = ClockedServer::start(root.path(), &jwks.url).await?;
        let prepared = AdminImportScenario::prepare(server.base_url()).await;
        let stopped = server.shutdown().await;
        stopped?;
        let prepared = prepared?;
        let server = ClockedServer::start(root.path(), &jwks.url).await?;
        let verified = prepared.verify_reopen(server.base_url()).await;
        let stopped = server.shutdown().await;
        stopped?;
        verified
    }
    .await;
    let closed = jwks.shutdown().await;
    closed?;
    result
}
