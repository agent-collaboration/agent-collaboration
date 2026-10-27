//! Compose and drain the production mail owner around a fixture's native reader actors.

use super::mail_service::{ApplicationFixture, MailDeliveryRuntime, MailPushDeliveryProps};
use fleet_sync_protocol::PeerId;
use interaction_storage::InteractionStore;
use std::{io, sync::Arc, time::Duration};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

type FixtureError = Box<dyn std::error::Error + Send + Sync>;

pub struct NativeMailOwner {
    pub interactions: Arc<InteractionStore>,
    mail: Arc<MailDeliveryRuntime>,
    shutdown: CancellationToken,
    running: JoinHandle<Result<(), durable_steps::StoreError>>,
}

impl NativeMailOwner {
    // Host proofs supply a prepared session instead.
    pub fn start(
        identity: ApplicationFixture,
        peer: PeerId,
        props: MailPushDeliveryProps,
    ) -> Result<(ApplicationFixture, Self), FixtureError> {
        Self::start_with_session(identity, peer, props, None)
    }

    // Host publisher proofs borrow their one actual prepared session.
    pub fn start_with_session(
        identity: ApplicationFixture,
        peer: PeerId,
        props: MailPushDeliveryProps,
        session: Option<collaboration_runtime::MailDeliverySession>,
    ) -> Result<(ApplicationFixture, Self), FixtureError> {
        let interactions = props.interactions.clone();
        let mail = Arc::new(MailDeliveryRuntime::new(peer, props, session)?);
        Self::start_prepared(identity, peer, interactions, mail)
    }

    // Unit quota proofs prepare the same runtime with smaller SDK limits.
    // This is the only runner, and identity validates the physical owner pair.
    pub fn start_prepared(
        identity: ApplicationFixture,
        peer: PeerId,
        interactions: Arc<InteractionStore>,
        mail: Arc<MailDeliveryRuntime>,
    ) -> Result<(ApplicationFixture, Self), FixtureError> {
        let identity = identity
            .with_interaction_store(interactions.clone())
            .with_installation_peer(peer)
            .map_err(io::Error::other)?
            .with_mail_delivery_runtime(mail.clone())
            .map_err(io::Error::other)?;
        let shutdown = CancellationToken::new();
        let worker_mail = mail.clone();
        let worker_shutdown = shutdown.clone();
        let running = tokio::spawn(async move { worker_mail.run(worker_shutdown).await });
        Ok((
            identity,
            Self {
                interactions,
                mail,
                shutdown,
                running,
            },
        ))
    }

    // Only lifecycle proofs inspect SDK custody through its owning runtime.
    pub fn runtime(&self) -> &MailDeliveryRuntime {
        &self.mail
    }

    // Host composition keeps the same owner behind its weak session port.
    pub fn runtime_handle(&self) -> Arc<MailDeliveryRuntime> {
        self.mail.clone()
    }

    // Only the shutdown lifecycle proof stops the owner before its client returns.
    pub fn begin_shutdown(&self) {
        self.shutdown.cancel();
    }

    // The lifecycle proof checks the actual tracked runtime task.
    pub fn is_stopped(&self) -> bool {
        self.running.is_finished()
    }

    /// Stop native work while a caller still owns the shared physical database.
    pub async fn stop(self) -> Result<Arc<InteractionStore>, FixtureError> {
        let Self {
            interactions,
            mail,
            shutdown,
            running,
        } = self;
        shutdown.cancel();
        let joined = tokio::time::timeout(Duration::from_secs(10), running).await?;
        joined??;
        drop(mail);
        Ok(interactions)
    }

    // Only interrupted-handover proofs abandon an active SDK drive.
    pub async fn abort_for_crash(self) -> Result<Arc<InteractionStore>, FixtureError> {
        let Self {
            interactions,
            mail,
            shutdown,
            running,
        } = self;
        running.abort();
        match running.await {
            Err(error) if error.is_cancelled() => {}
            _ => {
                return Err(
                    io::Error::other("SDK drive ended before the interruption checkpoint").into(),
                );
            }
        }
        shutdown.cancel();
        drop(mail);
        Ok(interactions)
    }

    /// Stop the served application and reader actors before closing their physical owner.
    // Host fixtures stop first, then check and close each store owner separately.
    pub async fn close(self) -> Result<(), FixtureError> {
        Arc::try_unwrap(self.stop().await?)
            .map_err(|_| io::Error::other("fixture retained the native interaction owner"))?
            .close()
            .await?;
        Ok(())
    }
}
