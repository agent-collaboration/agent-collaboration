pub use super::forwarded_post_wire::close;
use super::*;
use collaboration_central_conformance::{ConformanceClient, enroll_admitted_peer, fixture_policy};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use reqwest_websocket::WebSocket;
use std::{path::Path, time::Duration};

pub const SENDER_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440476";
pub const SENDER_CREDENTIAL: &str = "forward sender";
pub const HOLDER_CREDENTIAL: &str = "forward holder";
const HOLDER_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440477";
const SENDER_PEER: &str = "019f0000-0000-7000-8000-000000000476";
const HOLDER_PEER: &str = "019f0000-0000-7000-8000-000000000477";

pub async fn start(root: &Path, jwks: &str) -> ConformanceResult<TokioServer> {
    Ok(TokioServer::start_conformance(TokioSettings {
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
    .await?)
}

pub async fn arrange_holder(root: &Path, jwks: &str) -> ConformanceResult<DatabaseId> {
    let server = start(root, jwks).await?;
    let client = ConformanceClient::new(server.base_url())?;
    for (service, peer, credential) in [
        (SENDER_SERVICE, SENDER_PEER, SENDER_CREDENTIAL),
        (HOLDER_SERVICE, HOLDER_PEER, HOLDER_CREDENTIAL),
    ] {
        let machine = enroll_admitted_peer(
            &client,
            service,
            peer,
            credential,
            &durable_steps::Uuid::now_v7().to_string(),
        )
        .await?;
        close(machine.socket).await?;
    }
    server.shutdown().await?;
    let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
    // The public library registration task creates the real Copy and grant.
    // Its admission-gate stand-in does not prove normal Register composition.
    registry_setup::register_grant_history(
        root,
        configuration::registry_config()?,
        database,
        vec![(Epoch::new(1)?, PeerId::try_from(HOLDER_PEER.to_owned())?)],
    )
    .await?;
    Ok(database)
}

pub async fn connect(server: &TokioServer, credential: &str) -> ConformanceResult<WebSocket> {
    super::forwarded_post_wire::connect(server.base_url(), credential).await
}
