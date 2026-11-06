//! Compose and drain the production mail owner around a fixture's native reader actors.

pub use collaboration_service::NativeMailOwner;
use collaboration_service::{ApplicationFixture, MailDeliveryRuntime, MailPushDeliveryProps};
use fleet_sync_protocol::PeerId;
use interaction_storage::InteractionStore;
use std::{io, sync::Arc};

type FixtureError = Box<dyn std::error::Error + Send + Sync>;

// Host proofs supply a prepared session instead.
pub fn start(
    identity: ApplicationFixture,
    peer: PeerId,
    props: MailPushDeliveryProps,
) -> Result<(ApplicationFixture, NativeMailOwner), FixtureError> {
    start_with_session(identity, peer, props, None)
}

// Host publisher proofs borrow their one actual prepared session.
pub fn start_with_session(
    identity: ApplicationFixture,
    peer: PeerId,
    props: MailPushDeliveryProps,
    session: Option<collaboration_runtime::MailDeliverySession>,
) -> Result<(ApplicationFixture, NativeMailOwner), FixtureError> {
    let interactions = props.interactions.clone();
    let mail = Arc::new(MailDeliveryRuntime::new(peer, props, session)?);
    start_prepared(identity, peer, interactions, mail)
}

// Unit quota proofs prepare the same runtime with smaller SDK limits.
// This is the only runner, and identity validates the physical owner pair.
pub fn start_prepared(
    identity: ApplicationFixture,
    peer: PeerId,
    interactions: Arc<InteractionStore>,
    mail: Arc<MailDeliveryRuntime>,
) -> Result<(ApplicationFixture, NativeMailOwner), FixtureError> {
    start_prepared_after(identity, peer, interactions, mail, None)
}

pub fn start_deferred_with_session(
    identity: ApplicationFixture,
    peer: PeerId,
    props: MailPushDeliveryProps,
    session: Option<collaboration_runtime::MailDeliverySession>,
) -> Result<(ApplicationFixture, NativeMailOwner), FixtureError> {
    let interactions = props.interactions.clone();
    let mail = Arc::new(MailDeliveryRuntime::new(peer, props, session)?);
    let (release, waiting) = tokio::sync::oneshot::channel();
    start_prepared_after(identity, peer, interactions, mail, Some((release, waiting)))
}

fn start_prepared_after(
    identity: ApplicationFixture,
    peer: PeerId,
    interactions: Arc<InteractionStore>,
    mail: Arc<MailDeliveryRuntime>,
    deferred_start: Option<(
        tokio::sync::oneshot::Sender<()>,
        tokio::sync::oneshot::Receiver<()>,
    )>,
) -> Result<(ApplicationFixture, NativeMailOwner), FixtureError> {
    let identity = identity
        .with_interaction_store(interactions.clone())
        .with_installation_peer(peer)
        .map_err(io::Error::other)?
        .with_mail_delivery_runtime(mail.clone())
        .map_err(io::Error::other)?;
    Ok((
        identity,
        NativeMailOwner::start_prepared(interactions, mail, deferred_start),
    ))
}
