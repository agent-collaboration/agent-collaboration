//! The restore stays Loading while its caller publishes activation.

use provider_session_machines::{SessionGeneration, session_lifecycle_machine::LoadAttempt};
use std::collections::HashMap;
use tokio::sync::oneshot;

use super::{ExternalProviderRuntimeError, RestoredSessionAdmission};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RestorePublication {
    Published,
    Failed,
}

pub(super) struct RestorePublicationReceiver {
    pub(super) decision: oneshot::Receiver<RestorePublication>,
    pub(super) settled: oneshot::Sender<Result<(), ExternalProviderRuntimeError>>,
}

pub(super) struct RestorePublicationSender {
    decision: oneshot::Sender<RestorePublication>,
    settled: oneshot::Receiver<Result<(), ExternalProviderRuntimeError>>,
}

impl RestorePublicationSender {
    pub(super) fn channel() -> (Self, RestorePublicationReceiver) {
        let (decision_tx, decision_rx) = oneshot::channel();
        let (settled_tx, settled_rx) = oneshot::channel();
        (
            Self {
                decision: decision_tx,
                settled: settled_rx,
            },
            RestorePublicationReceiver {
                decision: decision_rx,
                settled: settled_tx,
            },
        )
    }

    async fn complete(
        self,
        decision: RestorePublication,
    ) -> Result<(), ExternalProviderRuntimeError> {
        // A dropped consumer also retires the restore. If its sender is
        // already gone, the settlement still carries the strongest result.
        let _sent = self.decision.send(decision);
        self.settled
            .await
            .map_err(|_| ExternalProviderRuntimeError::TransportFailure)?
    }
}

/// ACP restored the session. The connection holds its route provisionally
/// until publication is acknowledged. Dropping this handle retires it.
pub struct PreparedProviderRestore {
    pub admission: RestoredSessionAdmission,
    pub(super) publication: RestorePublicationSender,
}

impl PreparedProviderRestore {
    /// Opens input after the caller has published activation for this run.
    pub async fn activate(self) -> Result<RestoredSessionAdmission, ExternalProviderRuntimeError> {
        self.publication
            .complete(RestorePublication::Published)
            .await?;
        Ok(self.admission)
    }

    /// Disposes of the local route without waiting for a provider turn.
    pub async fn retire(self) -> Result<(), ExternalProviderRuntimeError> {
        self.publication.complete(RestorePublication::Failed).await
    }
}

/// The connection holds the SDK route itself, so a failure can drop it
/// before admitting any same-session retry.
pub(super) struct ProvisionalRestoreActor {
    pub(super) generation: SessionGeneration,
    pub(super) attempt: LoadAttempt,
    pub(super) session: agent_client_protocol::ActiveSession<'static, agent_client_protocol::Agent>,
    pub(super) item_projection: Option<crate::provider_item_projection::ProviderItemProjection>,
}

pub(super) fn take_provisional_restore(
    restores: &mut HashMap<String, ProvisionalRestoreActor>,
    session_id: &str,
    generation: SessionGeneration,
) -> Option<ProvisionalRestoreActor> {
    if restores
        .get(session_id)
        .is_some_and(|actor| actor.generation == generation)
    {
        restores.remove(session_id)
    } else {
        None
    }
}
