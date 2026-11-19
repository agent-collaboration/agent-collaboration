//! The link acknowledges durable inbox custody. Local handoff has its own task.
use crate::{
    MachineLocalStore, MachineLocalStoreError,
    inbox_store::{InboxStore, InboxStoreError, PendingAck, RecordDelivery},
};
use central_link_client::next_mark_after;
use router_clock::RuntimeClock;
use runtime_primitives::SyncTime;
use std::{collections::BTreeMap, future::Future, path::Path, pin::Pin, sync::Arc};
use tokio::sync::{Mutex, Notify};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use workspace_model::{
    MessageId,
    central_mail::{DeliveryAnswer, MessageRecord},
};

#[derive(Debug, thiserror::Error)]
#[error("central mail handoff is temporarily unavailable")]
pub struct CentralHandoffUnavailable;

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
    handoff: Arc<dyn CentralMessageHandoff>,
    uncommitted: BTreeMap<MessageId, MessageRecord>,
    replay_pending: bool,
}

impl CentralInbox {
    pub async fn open(
        path: &Path,
        handoff: Arc<dyn CentralMessageHandoff>,
    ) -> Result<Self, MachineLocalStoreError> {
        let store = InboxStore::new(MachineLocalStore::open(path).await?);
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
            changed: Arc::new(Notify::new()),
            handoff,
            uncommitted: BTreeMap::new(),
            replay_pending: false,
        })
    }

    pub(super) fn start_handoff(
        &self,
        clock: Arc<dyn RuntimeClock>,
        tasks: &TaskTracker,
        stop: CancellationToken,
    ) {
        let handoff = LocalHandoff {
            store: Arc::clone(&self.store),
            changed: Arc::clone(&self.changed),
            sink: Arc::clone(&self.handoff),
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
        let result = self
            .store
            .lock()
            .await
            .record_delivery(&message, DeliveryAnswer::Accepted, mark, now)
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
    sink: Arc<dyn CentralMessageHandoff>,
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
            if self.sink.handoff(&message).await.is_err() {
                handoff_retry = Some(next_mark_after(self.clock.now()));
                continue;
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
}
