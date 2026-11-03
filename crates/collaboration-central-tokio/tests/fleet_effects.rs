#![cfg(feature = "conformance")]

use board_sync_protocol::BoardReadScope;
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, enroll_admitted_peer, fixture_policy,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use message_board::{RequestEffectsRead, RequestEffectsRefusal, RequestId, WorkspaceId};
use std::time::Duration;

#[path = "native_http/support.rs"]
mod configuration;
#[path = "support/fleet_effects_scenarios.rs"]
mod scenarios;
use scenarios::read;

fn settings(root: &std::path::Path, jwks: &str) -> ConformanceResult<TokioSettings> {
    Ok(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.into(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
}

#[tokio::test]
async fn unknown_effects_read_is_correlated_and_keeps_the_same_session_usable()
-> ConformanceResult<()> {
    let jwks = JwksFixtureServer::start().await?;
    let root = tempfile::tempdir()?;
    let server = TokioServer::start_conformance(settings(root.path(), &jwks.url)?).await?;
    let result = async {
        let client = ConformanceClient::new(server.base_url())?;
        let mut peer = enroll_admitted_peer(
            &client,
            "550e8400-e29b-41d4-a716-4466554400f1",
            "019f0000-0000-7000-8000-00000000100d",
            "fleet-effects-reader",
            "019f0000-0000-7000-8000-000000000f11",
        )
        .await?;
        let result = async {
            for _ in 0..2 {
                let answer = read(
                    &mut peer.socket,
                    BoardReadScope::RequestEffects {
                        workspace: WorkspaceId::try_from(
                            "019f0000-0000-7000-8000-000000000f12".to_owned(),
                        )?,
                        request: RequestId::try_from(
                            "019f0000-0000-7000-8000-000000000f13".to_owned(),
                        )?,
                        after: None,
                    },
                )
                .await?;
                if !matches!(
                    answer,
                    RequestEffectsRead::Refused {
                        reason: RequestEffectsRefusal::UnknownRequest
                    }
                ) {
                    return Err("an unknown request did not give its typed read refusal".into());
                }
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let closed = peer
            .socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        result?;
        closed?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result
}

#[tokio::test]
async fn effects_pages_are_request_scoped_and_survive_a_whole_server_reopen()
-> ConformanceResult<()> {
    let jwks = JwksFixtureServer::start().await?;
    let root = tempfile::tempdir()?;
    let server = TokioServer::start_conformance(settings(root.path(), &jwks.url)?).await?;
    let result = async {
        scenarios::seed(server.base_url()).await?;
        scenarios::pages_and_refusals(server.base_url(), false).await
    }
    .await;
    let stopped = server.shutdown().await;
    stopped?;
    if let Err(error) = result {
        jwks.shutdown().await?;
        return Err(error);
    }
    let server = TokioServer::start_conformance(settings(root.path(), &jwks.url)?).await?;
    let result = scenarios::pages_and_refusals(server.base_url(), true).await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    result
}
