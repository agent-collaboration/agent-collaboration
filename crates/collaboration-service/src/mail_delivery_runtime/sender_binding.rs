//! Resolve sealed senders from the machine identity and its authenticated directory.
use collaboration_mail_protocol::MailSender;
use durable_steps::storage::TransactionScope;
use fleet_mail_delivery::{SenderBinding, SenderBindingFailure};
use fleet_protocol::{DatabaseId, DeliveryTypes, PeerId, Submitter};
use message_board::ServiceId;
use std::{collections::BTreeSet, sync::Mutex};

/// Lookup hints carry no authority; the supplied custody scope owns directory truth.
pub struct MachineSenderBinding {
    service: ServiceId,
    peer: PeerId,
    missing: Mutex<BTreeSet<ServiceId>>,
    requested: tokio::sync::Notify,
}

impl MachineSenderBinding {
    #[must_use]
    pub fn new(service: ServiceId, peer: PeerId) -> Self {
        Self {
            service,
            peer,
            missing: Mutex::new(BTreeSet::new()),
            requested: tokio::sync::Notify::new(),
        }
    }

    pub(super) async fn lookup_requested(&self) {
        self.requested.notified().await;
    }

    pub(super) fn take_missing_installations(
        &self,
    ) -> Result<BTreeSet<ServiceId>, durable_steps::StoreError> {
        let mut missing = self
            .missing
            .lock()
            .map_err(|_| durable_steps::StoreError::backend("sender lookup hints poisoned"))?;
        Ok(std::mem::take(&mut *missing))
    }
}

impl<S, D> SenderBinding<S, D> for MachineSenderBinding
where
    S: TransactionScope<Database = super::MailDatabase>,
    D: DeliveryTypes<Sender = MailSender>,
{
    #[expect(
        clippy::manual_async_fn,
        reason = "Explicit Send preserves higher-ranked custody scope futures."
    )]
    fn resolve_submitter<'a>(
        &'a self,
        scope: &'a mut S,
        sender: &'a MailSender,
    ) -> impl std::future::Future<Output = Result<Submitter, SenderBindingFailure>> + Send + 'a
    {
        async move {
            let service = match sender {
                MailSender::Project { project } => {
                    return DatabaseId::try_from(project.as_str().to_owned())
                        .map(Submitter::Project)
                        .map_err(|_| SenderBindingFailure::Conflict);
                }
                MailSender::Session(session) => &session.endpoint.service_id,
                MailSender::HumanUser { installation } => installation,
            };
            if service == &self.service {
                return Ok(Submitter::Machine(self.peer));
            }
            match interaction_storage::installation_peer_in_scope(scope, service).await {
                Ok(Some(peer)) => PeerId::try_from(peer.as_str().to_owned())
                    .map(Submitter::Machine)
                    .map_err(|_| SenderBindingFailure::Conflict),
                Ok(None) => {
                    self.missing
                        .lock()
                        .map_err(|_| SenderBindingFailure::Unavailable)?
                        .insert(service.clone());
                    self.requested.notify_one();
                    Err(SenderBindingFailure::Unknown)
                }
                Err(
                    interaction_storage::StorageError::InstallationPeerConflict
                    | interaction_storage::StorageError::InvalidStoredRecord,
                ) => Err(SenderBindingFailure::Conflict),
                Err(_) => Err(SenderBindingFailure::Unavailable),
            }
        }
    }
}
