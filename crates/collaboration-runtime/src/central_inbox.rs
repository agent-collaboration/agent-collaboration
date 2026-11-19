//! The link acknowledges durable inbox custody. Local handoff has its own task.
use crate::{
    MachineLocalStore, MachineLocalStoreError, OutboxStorage, OutboxStoreError,
    inbox_store::{InboxStore, InboxStoreError, PendingAck, RecordDelivery},
};
use central_link_client::{FlushOutcomeSink, SenderDeliveryOutcome, next_mark_after};
use router_clock::RuntimeClock;
use runtime_primitives::SyncTime;
use std::{collections::BTreeMap, future::Future, path::Path, pin::Pin, sync::Arc};
use tokio::sync::{Mutex, Notify};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use workspace_model::{
    Caller, MessageId,
    central_mail::{DeliveryAnswer, MessageBody, MessageRecord, MessageSender},
};

#[derive(Debug, thiserror::Error)]
#[error("central mail handoff is temporarily unavailable")]
pub struct CentralHandoffUnavailable;

#[derive(Debug, thiserror::Error)]
#[error("recipient session lookup is temporarily unavailable")]
pub struct CentralRecipientUnavailable;

pub trait CentralRecipientLookup: Send + Sync {
    fn answer<'call>(
        &'call self,
        message: &'call MessageRecord,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<DeliveryAnswer, CentralRecipientUnavailable>> + Send + 'call,
        >,
    >;
}

pub trait CentralMessageHandoff: Send + Sync {
    /// Ensure the message is in the local delivery path under its central
    /// id. Retrying an already committed handoff must have no new effect.
    fn handoff<'call>(
        &'call self,
        message: &'call MessageRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), CentralHandoffUnavailable>> + Send + 'call>>;
}

pub struct CentralInbox {
    store: Arc<Mutex<InboxStore>>,
    changed: Arc<Notify>,
    recipients: Arc<dyn CentralRecipientLookup>,
    handoff: Option<Arc<dyn CentralMessageHandoff>>,
    uncommitted: BTreeMap<MessageId, MessageRecord>,
    replay_pending: bool,
}

impl CentralInbox {
    pub async fn open(
        path: &Path,
        recipients: Arc<dyn CentralRecipientLookup>,
        handoff: Option<Arc<dyn CentralMessageHandoff>>,
    ) -> Result<Self, MachineLocalStoreError> {
        let store = InboxStore::new(MachineLocalStore::open(path).await?);
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
            changed: Arc::new(Notify::new()),
            recipients,
            handoff,
            uncommitted: BTreeMap::new(),
            replay_pending: false,
        })
    }

    pub(super) fn start_handoff(
        &self,
        clock: Arc<dyn RuntimeClock>,
        outbox: Arc<Mutex<dyn OutboxStorage>>,
        outcomes: Arc<dyn FlushOutcomeSink>,
        tasks: &TaskTracker,
        stop: CancellationToken,
    ) {
        let handoff = LocalHandoff {
            store: Arc::clone(&self.store),
            changed: Arc::clone(&self.changed),
            sink: self.handoff.clone(),
            outbox,
            outcomes,
            clock,
        };
        tasks.spawn(async move {
            tokio::select! {
                () = stop.cancelled() => {}
                () = handoff.run() => {}
            }
        });
    }

    pub(super) async fn receive(&mut self, message: MessageRecord, mark: SyncTime, now: SyncTime) {
        let message = self
            .uncommitted
            .remove(&message.message_id)
            .unwrap_or(message);
        self.admit(message, mark, now).await;
    }

    async fn admit(&mut self, message: MessageRecord, mark: SyncTime, now: SyncTime) {
        let duplicate = self
            .store
            .lock()
            .await
            .reoffer_duplicate(&message.message_id, mark, now)
            .await;
        match duplicate {
            Ok(true) => return,
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(%error, "inbox: unable to check custody");
                self.uncommitted.insert(message.message_id.clone(), message);
                return;
            }
        }
        let Ok(answer) = self.recipients.answer(&message).await else {
            self.uncommitted.insert(message.message_id.clone(), message);
            return;
        };
        let result = self
            .store
            .lock()
            .await
            .record_delivery(&message, answer, mark, now)
            .await;
        match result {
            Ok(RecordDelivery::Inserted) => self.changed.notify_one(),
            Ok(RecordDelivery::Duplicate) => {}
            Err(error) => {
                tracing::warn!(%error, "inbox: initial write unavailable, retaining delivery for retry");
                self.uncommitted.insert(message.message_id.clone(), message);
            }
        }
    }

    pub(super) async fn reoffer_recent_acks(&mut self, mark: SyncTime, now: SyncTime) {
        let result = self.store.lock().await.reoffer_recent_acks(mark, now).await;
        self.replay_pending = result.is_err();
        if let Err(error) = result {
            tracing::warn!(%error, "inbox: could not reoffer recent Acks, retrying next minute");
        }
    }

    pub(super) async fn retry_pending(&mut self, mark: SyncTime, now: SyncTime) {
        if self.replay_pending {
            self.reoffer_recent_acks(mark, now).await;
        }
        for message in std::mem::take(&mut self.uncommitted).into_values() {
            self.admit(message, mark, now).await;
        }
    }

    pub(super) fn needs_retry(&self) -> bool {
        self.replay_pending || !self.uncommitted.is_empty()
    }

    pub(super) async fn earliest_due(&self) -> Result<Option<SyncTime>, InboxStoreError> {
        Ok(self
            .store
            .lock()
            .await
            .pending_acks()
            .await?
            .into_iter()
            .map(|ack| ack.flush_at)
            .min())
    }

    pub(super) async fn acks_due(
        &self,
        mark: SyncTime,
    ) -> Result<Vec<PendingAck>, InboxStoreError> {
        self.store.lock().await.acks_due(mark).await
    }

    pub(super) async fn mark_ack_sent(
        &self,
        ack: &PendingAck,
        now: SyncTime,
    ) -> Result<(), InboxStoreError> {
        self.store
            .lock()
            .await
            .mark_ack_sent(&ack.message_id, now)
            .await?;
        self.changed.notify_one();
        Ok(())
    }
}

struct LocalHandoff {
    store: Arc<Mutex<InboxStore>>,
    changed: Arc<Notify>,
    sink: Option<Arc<dyn CentralMessageHandoff>>,
    outbox: Arc<Mutex<dyn OutboxStorage>>,
    outcomes: Arc<dyn FlushOutcomeSink>,
    clock: Arc<dyn RuntimeClock>,
}

impl LocalHandoff {
    async fn run(self) {
        loop {
            let now = self.clock.now();
            let retry_at = match self.drain(now).await {
                Ok(next) => next,
                Err(error) => {
                    tracing::warn!(%error, "inbox: local handoff or pruning unavailable");
                    Some(next_mark_after(now))
                }
            };
            let Some(retry_at) = retry_at else {
                self.changed.notified().await;
                continue;
            };
            tokio::select! {
                () = self.changed.notified() => {}
                () = self.clock.sleep_until(self.clock.wall_deadline(retry_at)) => {}
            }
        }
    }

    async fn drain(&self, now: SyncTime) -> Result<Option<SyncTime>, InboxStoreError> {
        let messages = {
            let mut store = self.store.lock().await;
            store.prune_ended(now).await?;
            store.pending_handoffs().await?
        };
        let mut handoff_retry = None;
        for message in messages {
            match &message.body {
                MessageBody::DeliveryOutcome {
                    message_id,
                    request_id,
                    outcome,
                } => {
                    if message.sender == MessageSender::Central
                        && let Err(error) = self
                            .handoff_sender_outcome(
                                &Caller::Session(message.target.clone()),
                                &SenderDeliveryOutcome {
                                    inbox_message_id: message.message_id.clone(),
                                    message_id: message_id.clone(),
                                    request_id: request_id.clone(),
                                    outcome: *outcome,
                                },
                            )
                            .await
                    {
                        tracing::warn!(%error, "inbox: sender outcome handoff unavailable");
                        handoff_retry = Some(next_mark_after(self.clock.now()));
                        continue;
                    }
                }
                _ => {
                    let Some(sink) = &self.sink else { continue };
                    if sink.handoff(&message).await.is_err() {
                        handoff_retry = Some(next_mark_after(self.clock.now()));
                        continue;
                    }
                }
            }
            self.store
                .lock()
                .await
                .record_handoff(&message.message_id, self.clock.now())
                .await?;
        }
        let prune = self.store.lock().await.next_prune().await?;
        Ok(handoff_retry.into_iter().chain(prune).min())
    }

    async fn handoff_sender_outcome(
        &self,
        caller: &Caller,
        notice: &SenderDeliveryOutcome,
    ) -> Result<(), OutboxStoreError> {
        let stored = self
            .outbox
            .lock()
            .await
            .record_sender_delivery(caller, notice, self.clock.now())
            .await;
        match stored {
            Ok(true) => self.outcomes.delivery_outcome(caller, notice),
            Ok(false) => {}
            Err(error @ OutboxStoreError::DeliveryOutcomeMismatch { .. }) => {
                tracing::warn!(%error, "inbox: sender outcome conflicts with its local request");
            }
            Err(error) => return Err(error),
        }
        Ok(())
    }
}
