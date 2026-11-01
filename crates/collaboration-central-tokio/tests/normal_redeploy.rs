//! Preserve the old normal-state oracle at real public session/owner boundaries.
#![cfg(feature = "conformance")]
#[path = "native_http/support.rs"]
mod configuration;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;
#[path = "normal_redeploy/grant_readback.rs"]
mod grant_readback;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/imported_board_fixture.rs"]
mod imported_board_fixture;

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
#[path = "support/owner_registry_scenarios.rs"]
mod owner_registry_scenarios;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "mail_submit_http/registry_setup.rs"]
mod registry_setup;
#[path = "normal_redeploy/state.rs"]
mod state;
#[path = "normal_redeploy/transport.rs"]
mod transport;

use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, JwksFixtureServer, enroll_admitted_peer, fixture_policy,
    machine_assertion,
};
use collaboration_central_core::RegistryTime;
use collaboration_central_tokio::{TokioServer, TokioSettings};
use creation_scenarios as creation;
use durable_steps::Uuid;
use fleet_sync_protocol::*;
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::{Value, json};
use std::time::Duration;

fn settings(root: &std::path::Path, keys: &JwksFixtureServer) -> ConformanceResult<TokioSettings> {
    Ok(TokioSettings {
        root: root.into(),
        access_policy: fixture_policy(),
        jwks_url: keys.url.clone(),
        registry: configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: PeerId::try_from("019f0000-0000-7000-8000-000000005240".to_owned())?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
}

#[tokio::test]
async fn surviving_normal_socket_is_read_before_the_honest_reconnect() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let databases = imported_board_fixture::arrange_normal_redeploy_boards(root.path()).await?;
    let keys = JwksFixtureServer::start().await?;
    let server = TokioServer::start_conformance(settings(root.path(), &keys)?).await?;
    let proof = async {
        let mut prepared =
            state::NormalRedeployState::prepare(server.base_url(), databases).await?;
        let result = prepared
            .verify_surviving(server.base_url(), server.conformance_platform())
            .await;
        let closed = prepared.close().await;
        result?;
        closed?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = server.shutdown().await;
    let keys = keys.shutdown().await;
    proof?;
    stopped?;
    keys?;
    grant_readback::assert_no_granted_epoch_three(root.path(), databases[0]).await?;
    Ok(())
}

#[tokio::test]
async fn staging_normal_rows_pass_across_local_server_reopen() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let [
        project,
        second,
        moving,
        forwarding,
        owner_loss,
        renewal,
        window,
    ] = imported_board_fixture::arrange_normal_redeploy_corpus(root.path()).await?;
    let keys = JwksFixtureServer::start().await?;
    let proof = async {
        let first = TokioServer::start_conformance(settings(root.path(), &keys)?).await?;
        let prepared = async {
            state::run_preludes(
                first.base_url(),
                first.conformance_platform(),
                [moving, forwarding, owner_loss, renewal, window],
            )
            .await?;
            state::NormalRedeployState::prepare(first.base_url(), [project, second]).await
        }
        .await;
        let stopped = first.shutdown().await;
        let prepared = prepared?;
        stopped?;
        let closed = prepared.observe_server_close().await?;
        let second = TokioServer::start_conformance(settings(root.path(), &keys)?).await?;
        println!(
            "NORMAL_REDEPLOY_STAGE second_server_started_on_same_root_before_reconnect passed"
        );
        let resumed = closed.verify_after_reopen(second.base_url()).await;
        let stopped = second.shutdown().await;
        resumed?;
        stopped?;
        grant_readback::assert_no_granted_epoch_three(root.path(), project).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let keys = keys.shutdown().await;
    proof?;
    keys?;
    Ok(())
}
