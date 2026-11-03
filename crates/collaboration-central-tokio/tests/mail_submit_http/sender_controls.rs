//! Native entry points for the same sender-control bodies used on workerd.
use super::sender_control_scenarios;
use super::*;

async fn fixture() -> ConformanceResult<(tempfile::TempDir, JwksFixtureServer, TokioServer)> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start_conformance(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    Ok((root, jwks, server))
}
async fn finish(
    server: TokioServer,
    jwks: JwksFixtureServer,
    proof: ConformanceResult<()>,
) -> ConformanceResult<()> {
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    proof
}

#[tokio::test]
async fn raw_submit_is_refused_without_custody_and_the_same_socket_remains_usable()
-> ConformanceResult<()> {
    let (_root, jwks, server) = fixture().await?;
    let proof=sender_control_scenarios::raw_submit_is_refused_without_custody_and_the_same_socket_remains_usable(server.base_url()).await;
    finish(server, jwks, proof).await
}

#[tokio::test]
async fn sender_query_and_settlement_preserve_existing_custody_and_pin_unknown_identity()
-> ConformanceResult<()> {
    let (_root, jwks, server) = fixture().await?;
    let proof=sender_control_scenarios::sender_query_and_settlement_preserve_existing_custody_and_pin_unknown_identity(server.base_url()).await;
    finish(server, jwks, proof).await
}

#[tokio::test]
async fn retirement_uses_the_verified_sender_and_replays_its_original_call_receipt()
-> ConformanceResult<()> {
    let (_root, jwks, server) = fixture().await?;
    let proof=sender_control_scenarios::retirement_uses_the_verified_sender_and_replays_its_original_call_receipt(server.base_url()).await;
    finish(server, jwks, proof).await
}
