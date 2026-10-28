//! Actual SDK session handle with no pump, socket or welcomed carrier.
use board_fleet_sync::{AccessServiceToken, MachineSessionStore, ReqwestSessionTransport};
use board_sync_protocol::BoardConsumerProtocol;
use collaboration_runtime::MailDeliverySession;
use fleet_sync_protocol::{PeerId, SchemaSupport};
use fleet_sync_session::{Endpoint, PeerCredential, SessionClient};
use fleet_sync_tokio_runner::SessionPump;
use runtime_primitives::{Backoff, UuidV7Ids};
use std::{path::Path, time::Duration};

pub async fn closed_session(
    root: &Path,
    peer: PeerId,
) -> Result<MailDeliverySession, Box<dyn std::error::Error + Send + Sync>> {
    let bound = Duration::from_secs(5);
    let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
        ReqwestSessionTransport::new(AccessServiceToken::new("fixture", "fixture")?, bound, bound)?,
        MachineSessionStore::open(root).await?,
        &UuidV7Ids,
        SchemaSupport { max_version: 1 },
        Backoff {
            initial: bound,
            max: bound,
            factor: 2,
        },
    )
    .await?;
    let (requests, events, pump) = SessionPump::new(
        client,
        Endpoint::new("ws://127.0.0.1:1/session")?,
        PeerCredential::new("Bearer offline-fixture")?,
        bound,
    );
    drop(events);
    drop(pump);
    let (_, welcomed) = tokio::sync::watch::channel(false);
    Ok(MailDeliverySession {
        requests,
        welcomed,
        peer,
        relay: PeerId::new(uuid::Uuid::now_v7())?,
    })
}
