#![cfg(feature = "conformance")]
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, fixture_policy,
    run_fleet_mail_observer_conformance,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};

#[tokio::test]
async fn owner_observes_one_committed_item_and_refusal_preserves_the_count() -> ConformanceResult<()>
{
    let jwks = JwksFixtureServer::start().await?;
    let root = tempfile::tempdir()?;
    let server = TokioServer::start(TokioSettings {
        legacy_organization: collaboration_central_conformance::legacy_organization(),
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: crate::configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: "019f0000-0000-7000-8000-000000005240"
                .to_owned()
                .try_into()?,
        },
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    })
    .await?;
    let proof =
        run_fleet_mail_observer_conformance(&ConformanceClient::new(server.base_url())?).await;
    let stopped = server.shutdown().await;
    let keys = jwks.shutdown().await;
    proof?;
    stopped?;
    keys?;
    Ok(())
}
