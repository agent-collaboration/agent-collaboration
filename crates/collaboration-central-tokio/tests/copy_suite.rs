//! Shared copy assertions replace actor eviction with whole native server restart.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, fixture_policy,
    run_composed_copy_suite,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use std::sync::Arc;

#[tokio::test]
async fn shared_copy_assertions_use_real_storage_sessions_and_physical_restarts()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let root = Arc::new(root);
    let settings = {
        let root = root.clone();
        let url = jwks.url.clone();
        move || {
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(TokioSettings {
                delivery: collaboration_central_tokio::DeliveryConfig {
                    relay_peer: fleet_sync_protocol::PeerId::try_from(
                        "019f0000-0000-7000-8000-000000005240".to_owned(),
                    )?,
                },
                root: root.path().into(),
                access_policy: fixture_policy(),
                jwks_url: url.clone(),
                registry: configuration::registry_config()?,
                holder_answer_timeout: std::time::Duration::from_secs(2),
                copy_ack_wait: std::time::Duration::from_secs(2),
            })
        }
    };
    let first = TokioServer::start(settings()?).await?;
    let client = ConformanceClient::new(first.base_url())?;
    let server = Arc::new(tokio::sync::Mutex::new(Some(first)));
    let proof = run_composed_copy_suite(client, {
        let server = server.clone();
        move || {
            let server = server.clone();
            let settings = settings.clone();
            async move {
                let mut slot = server.lock().await;
                slot.take()
                    .ok_or("native restart without server")?
                    .shutdown()
                    .await?;
                let next = TokioServer::start(settings()?).await?;
                let client = ConformanceClient::new(next.base_url())?;
                *slot = Some(next);
                Ok(client)
            }
        }
    })
    .await;
    if let Some(server) = server.lock().await.take() {
        server.shutdown().await?;
    }
    jwks.shutdown().await?;
    proof
}
