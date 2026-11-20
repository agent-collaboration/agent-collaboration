use router_clock::RuntimeClock;
#[path = "native_mail_readiness.rs"]
mod native_mail_readiness;
use super::{
    reader_delivery_owner::{ReaderDeliveryCommand, ReaderDeliveryOwner, ReaderDeliveryOwnerProps},
    subscription_push::{SubscriptionPushStore, SubscriptionPushStoreProps},
};
use collaboration_contracts::MachineIdentity;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_contracts::TargetPresenceProbe;
use collaboration_protocol::{PushId, PushRecord, SessionRef};
use message_board::{BoardError, Identity};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::instrument::WithSubscriber;

pub struct SubscriptionDeliveryServiceProps {
    pub push_store: Arc<interaction_storage::InteractionStore>,
    pub delivery: Arc<dyn SessionMessageDelivery>,
    pub presence: Arc<dyn TargetPresenceProbe>,
    pub machine_identity: MachineIdentity,
    pub clock: Arc<dyn RuntimeClock>,
}

#[cfg(test)]
#[derive(Clone, Debug)]
pub(super) enum OwnerObservation {
    Started,
    Sleeping,
    DirectMessageQueued,
    MailAttemptQueued(PushId),
    NativeHeldChecked,
    /// The reader took a signal that its held mail's target may have loaded, and
    /// whether it owed another presence check.
    HeldMailSignalTaken,
    /// A pass failed at this stage and armed its backoff retry.
    PassFailed,
    DirectMessageHeld,
    DirectMessageSettled,
    PushSettlementRetry,
    Stopped,
}

pub(super) type NativeMailReadiness =
    Arc<std::sync::OnceLock<std::sync::Weak<crate::MailDeliveryRuntime>>>;

struct ServiceInner {
    native_mail: NativeMailReadiness,
    /// Set by the Host before start; readers created without it check held mail only
    /// when they first see it.
    held_mail_signals: std::sync::OnceLock<super::HeldMailSignals>,
    push: Arc<SubscriptionPushStore>,
    presence: Arc<dyn TargetPresenceProbe>,
    clock: Arc<dyn RuntimeClock>,
    owners: Mutex<HashMap<Identity, mpsc::Sender<ReaderDeliveryCommand>>>,
    start_gate: Mutex<bool>,
    shutdown: CancellationToken,
    tasks: TaskTracker,
    #[cfg(test)]
    observations: tokio::sync::broadcast::Sender<OwnerObservation>,
}

#[derive(Clone)]
enum ServiceHandleLifetime {
    Owner(Arc<()>),
    Background,
}

#[derive(Clone)]
pub struct SubscriptionDeliveryService {
    inner: Arc<ServiceInner>,
    lifetime: ServiceHandleLifetime,
}

impl SubscriptionDeliveryService {
    /// Gives readers the signals that a held message's target may have loaded. Set once,
    /// before start.
    pub fn attach_held_mail_signals(&self, signals: super::HeldMailSignals) -> Result<(), String> {
        self.inner
            .held_mail_signals
            .set(signals)
            .map_err(|_| "reader held-mail signals are already attached".to_owned())
    }

    #[must_use]
    pub fn runtime_clock(&self) -> Arc<dyn RuntimeClock> {
        Arc::clone(&self.inner.clock)
    }

    pub fn new(props: SubscriptionDeliveryServiceProps) -> Self {
        #[cfg(test)]
        let (observations, _) = tokio::sync::broadcast::channel(1024);
        let shutdown = CancellationToken::new();
        let push = Arc::new(SubscriptionPushStore::new(SubscriptionPushStoreProps {
            store: props.push_store,
            delivery: props.delivery,
            machine: props.machine_identity,
            clock: props.clock.clone(),
            shutdown: shutdown.clone(),
            #[cfg(test)]
            observations: observations.clone(),
        }));
        Self {
            lifetime: ServiceHandleLifetime::Owner(Arc::new(())),
            inner: Arc::new(ServiceInner {
                native_mail: Arc::new(std::sync::OnceLock::new()),
                held_mail_signals: std::sync::OnceLock::new(),
                push,
                presence: props.presence,
                clock: props.clock,
                owners: Mutex::new(HashMap::new()),
                start_gate: Mutex::new(false),
                shutdown,
                tasks: TaskTracker::new(),
                #[cfg(test)]
                observations,
            }),
        }
    }

    pub async fn start(&self) -> Result<(), BoardError> {
        let mut started = self.inner.start_gate.lock().await;
        if self.inner.shutdown.is_cancelled() {
            return Err(BoardError::board_unavailable());
        }
        if *started {
            return Ok(());
        }
        let dm_targets = match self.inner.push.restore_direct_messages().await {
            Ok(targets) => targets,
            Err(error) if recovery_connection_failed(&error) => {
                tracing::warn!(%error, "direct-message recovery connection unavailable");
                return Err(BoardError::board_unavailable());
            }
            Err(error) => {
                tracing::warn!(%error, "direct-message recovery failed; new mail remains available");
                Vec::new()
            }
        };
        *started = true;
        for target in dm_targets {
            self.ensure_owner(reader_for_target(&target)?).await?;
        }
        Ok(())
    }

    /// The record already exists: route its known-unsent work through the target's owner.
    pub async fn deliver_direct_message(
        &self,
        target: SessionRef,
        push_id: PushId,
    ) -> Result<PushRecord, BoardError> {
        self.require_started().await?;
        let reader = reader_for_target(&target)?;
        loop {
            let (reply, response) = oneshot::channel();
            let sender = self.ensure_owner(reader.clone()).await?;
            if sender
                .send(ReaderDeliveryCommand::DmQueued {
                    push_id: push_id.clone(),
                    reply,
                })
                .await
                .is_err()
            {
                continue;
            }
            #[cfg(test)]
            let _ = self
                .inner
                .observations
                .send(OwnerObservation::DirectMessageQueued);
            match response.await {
                Ok(result) => return result,
                Err(_) => {
                    self.require_started().await?;
                }
            }
        }
    }

    /// The caller owns admission and outcome persistence. A lost response after
    /// acceptance is Unknown; this method never requeues an accepted attempt.
    pub async fn attempt_mail_once(
        &self,
        kind: collaboration_protocol::PushKind,
        request: collaboration_contracts::layer_zero::DeliveryRequest,
    ) -> collaboration_contracts::NativeDeliveryResult {
        use collaboration_contracts::NativeDeliveryResult;
        use collaboration_contracts::NativeRefusalCode;
        use collaboration_contracts::UnsentCause;
        use collaboration_protocol::PushKind;
        let not_started = |retryable, reason: &str, cause| {
            NativeDeliveryResult::not_submitted(reason, retryable, cause)
        };
        let unavailable = UnsentCause::Holdable {
            otherwise: NativeRefusalCode::CarrierUnavailable,
        };
        if kind != PushKind::DirectMessage {
            return not_started(false, "invalid native mail kind", UnsentCause::InvalidInput);
        }
        if &request.target.endpoint.service_id != self.inner.push.machine.service_id() {
            return not_started(
                false,
                "mail target belongs to another Router",
                UnsentCause::InvalidInput,
            );
        }
        if self.require_started().await.is_err() {
            return not_started(
                true,
                "mail carrier is unavailable before the attempt",
                unavailable,
            );
        }
        let Ok(reader) = reader_for_target(&request.target) else {
            return not_started(
                false,
                "invalid native mail target",
                UnsentCause::InvalidInput,
            );
        };
        let Ok(sender) = self.ensure_owner(reader).await else {
            return not_started(
                true,
                "mail carrier closed before accepting the attempt",
                unavailable,
            );
        };
        #[cfg(test)]
        let push_id = request.payload.push_id.clone();
        let (reply, response) = oneshot::channel();
        let command = ReaderDeliveryCommand::MailAttempt {
            kind,
            request: Box::new(request),
            reply,
        };
        let sent = tokio::select! {
            ()=self.inner.shutdown.cancelled()=>return not_started(true,"mail carrier closed before accepting the attempt", unavailable),
            sent=sender.send(command)=>sent,
        };
        if sent.is_err() {
            return not_started(
                true,
                "mail carrier closed before accepting the attempt",
                unavailable,
            );
        }
        #[cfg(test)]
        let _ = self
            .inner
            .observations
            .send(OwnerObservation::MailAttemptQueued(push_id));
        tokio::select! {
            ()=self.inner.shutdown.cancelled()=>NativeDeliveryResult::unknown(),
            receipt=response=>receipt.unwrap_or_else(|_|NativeDeliveryResult::unknown()),
        }
    }

    pub fn cancel(&self) {
        self.inner.shutdown.cancel();
    }
    pub async fn shutdown(&self) {
        self.cancel();
        self.inner.tasks.close();
        self.inner.tasks.wait().await;
        self.inner.owners.lock().await.clear();
    }

    async fn require_started(&self) -> Result<(), BoardError> {
        if !*self.inner.start_gate.lock().await || self.inner.shutdown.is_cancelled() {
            Err(BoardError::board_unavailable())
        } else {
            Ok(())
        }
    }

    async fn ensure_owner(
        &self,
        reader: Identity,
    ) -> Result<mpsc::Sender<ReaderDeliveryCommand>, BoardError> {
        if self.inner.shutdown.is_cancelled() {
            return Err(BoardError::board_unavailable());
        }
        // Keep the map gate through spawn: no Reader can acquire two selection owners.
        let mut owners = self.inner.owners.lock().await;
        if let Some(sender) = owners
            .get(&reader)
            .filter(|sender| !sender.is_closed())
            .cloned()
        {
            return Ok(sender);
        }
        let (sender, commands) = mpsc::channel(64);
        let owner = ReaderDeliveryOwner::new(ReaderDeliveryOwnerProps {
            native_mail: self.inner.native_mail.clone(),
            reader: reader.clone(),
            push: self.inner.push.clone(),
            presence: self.inner.presence.clone(),
            clock: self.inner.clock.clone(),
            shutdown: self.inner.shutdown.clone(),
            commands,
            held_mail: super::held_mail_signals::HeldMailWatch::new(
                &self
                    .inner
                    .held_mail_signals
                    .get()
                    .cloned()
                    .unwrap_or_default(),
            ),
            #[cfg(test)]
            observations: self.inner.observations.clone(),
        });
        owners.insert(reader.clone(), sender.clone());
        let inner = self.inner.clone();
        let owner_sender = sender.clone();
        self.inner.tasks.spawn(
            async move {
                Box::pin(owner.run()).await;
                let mut owners = inner.owners.lock().await;
                if owners
                    .get(&reader)
                    .is_some_and(|current| current.same_channel(&owner_sender))
                {
                    owners.remove(&reader);
                }
            }
            .with_current_subscriber(),
        );
        Ok(sender)
    }

    #[cfg(test)]
    pub(super) fn observe_owners(&self) -> tokio::sync::broadcast::Receiver<OwnerObservation> {
        self.inner.observations.subscribe()
    }
}

impl Drop for SubscriptionDeliveryService {
    fn drop(&mut self) {
        if matches!(&self.lifetime,ServiceHandleLifetime::Owner(lifetime) if Arc::strong_count(lifetime) == 1)
        {
            self.cancel();
        }
    }
}

fn reader_for_target(target: &SessionRef) -> Result<Identity, BoardError> {
    let session = serde_json::from_value(
        serde_json::to_value(target).map_err(|_| BoardError::board_unavailable())?,
    )
    .map_err(|_| BoardError::board_unavailable())?;
    Ok(Identity::Session { session })
}

fn recovery_connection_failed(error: &interaction_storage::StorageError) -> bool {
    if let interaction_storage::StorageError::Database(sqlx::Error::Database(database)) = error {
        // SQLite extended error codes retain the primary connection/storage class.
        return database
            .code()
            .and_then(|code| code.parse::<i32>().ok())
            .is_some_and(|code| matches!(code & 0xff, 7 | 10 | 11 | 14 | 26));
    }
    matches!(
        error,
        interaction_storage::StorageError::Database(
            sqlx::Error::Io(_)
                | sqlx::Error::Tls(_)
                | sqlx::Error::Protocol(_)
                | sqlx::Error::WorkerCrashed
                | sqlx::Error::PoolClosed
                | sqlx::Error::PoolTimedOut
        )
    )
}
