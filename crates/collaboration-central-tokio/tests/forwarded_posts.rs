//! Real sessions preserve forwarding provenance and opaque request/answer bodies.
#![cfg(feature = "conformance")]
#![allow(clippy::panic_in_result_fn)]

#[path = "native_http/support.rs"]
mod configuration;
#[path = "support/forwarded_post_lifecycle_oracles.rs"]
mod forwarded_post_lifecycle_oracles;
#[path = "support/forwarded_post_oracles.rs"]
mod forwarded_post_oracles;
#[path = "support/forwarded_post_wire.rs"]
mod forwarded_post_wire;
#[path = "forwarded_posts/identity.rs"]
mod identity;
#[path = "forwarded_posts/lifecycle.rs"]
mod lifecycle;
#[path = "forwarded_posts/reconnect.rs"]
mod reconnect;
#[path = "mail_submit_http/registry_setup.rs"]
mod registry_setup;
#[allow(
    dead_code,
    reason = "Shared fixture also supports publication fault cases."
)]
#[path = "session_publication/support.rs"]
mod session_support;
#[path = "forwarded_posts/support.rs"]
mod support;

use board_sync_protocol::*;
use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer};
use fleet_sync_protocol::*;
use message_board::{ActorOrigin, ServiceId};
use support::*;

#[tokio::test]
async fn a_forward_preserves_verified_origin_and_returns_the_holders_opaque_answer()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let database = arrange_holder(root.path(), &jwks.url)
        .await
        .map_err(|error| format!("arrange actual grant and admissions: {error}"))?;
    let server = start(root.path(), &jwks.url).await?;
    let proof = async {
        let mut sender = connect(&server, SENDER_CREDENTIAL)
            .await
            .map_err(|error| format!("reconnect sender: {error}"))?;
        let mut holder = connect(&server, HOLDER_CREDENTIAL)
            .await
            .map_err(|error| format!("reconnect holder: {error}"))?;
        forwarded_post_oracles::preserves_origin_and_opaque_reply(
            &mut sender,
            &mut holder,
            database,
            ActorOrigin {
                installation: ServiceId::try_from(SENDER_SERVICE.to_owned())?,
                machine: SENDER_CREDENTIAL.to_owned().try_into()?,
            },
        )
        .await?;
        close(sender).await?;
        close(holder).await?;
        Ok(())
    }
    .await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}

#[tokio::test]
async fn a_disconnected_holder_is_reported_with_its_registry_presence() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let database = arrange_holder(root.path(), &jwks.url)
        .await
        .map_err(|error| format!("arrange actual grant and admissions: {error}"))?;
    let server = start(root.path(), &jwks.url).await?;
    let proof = async {
        // The holder was admitted during setup, but has no physical connection
        // on this server instance. Its actual Registry grant remains stored.
        let mut sender = connect(&server, SENDER_CREDENTIAL)
            .await
            .map_err(|error| format!("reconnect sender: {error}"))?;
        forwarded_post_oracles::disconnected_holder_has_registry_presence(&mut sender, database)
            .await?;
        close(sender).await?;
        Ok(())
    }
    .await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}
