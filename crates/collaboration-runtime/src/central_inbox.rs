//! The link owns durable inbox admission and Acks. The Host supplies the
//! handoff into its existing session delivery service.
use crate::{
    MachineLocalStore, MachineLocalStoreError,
    inbox_store::{InboxStore, InboxStoreError, PendingAck, RecordDelivery},
};
use runtime_primitives::SyncTime;
use std::{future::Future, path::Path, pin::Pin, sync::Arc};
use workspace_model::central_mail::{DeliveryAnswer, MessageRecord};

#[derive(Debug, thiserror::Error)]
#[error("central mail handoff is temporarily unavailable")]
pub struct CentralHandoffUnavailable;

pub trait CentralMessageHandoff: Send + Sync {
    /// Ensure the message is in the local delivery path under its central
    /// id. A previously committed handoff must return the same answer.
    fn handoff<'call>(
        &'call self,
        message: &'call MessageRecord,
    ) -> Pin<Box<dyn Future<Output = Result<DeliveryAnswer, CentralHandoffUnavailable>> + Send + 'call>>;
}

pub struct CentralInbox {
    store: InboxStore,
    handoff: Arc<dyn CentralMessageHandoff>,
}

impl CentralInbox {
    pub async fn open(
        path: &Path,
        handoff: Arc<dyn CentralMessageHandoff>,
    ) -> Result<Self, MachineLocalStoreError> {
        let store = InboxStore::new(MachineLocalStore::open(path).await?);
        Ok(Self { store, handoff })
    }

    pub(super) async fn receive(
        &mut self,
        message: &MessageRecord,
        mark: SyncTime,
        now: SyncTime,
    ) -> Result<(), InboxStoreError> {
        if self.store.record_delivery(message, mark, now).await? == RecordDelivery::Inserted {
            self.handoff_message(message, mark, now).await?;
        }
        Ok(())
    }

    pub(super) async fn recover_handoffs(
        &mut self,
        mark: SyncTime,
        now: SyncTime,
    ) -> Result<(), InboxStoreError> {
        for message in self.store.pending_handoffs().await? {
            self.handoff_message(&message, mark, now).await?;
        }
        Ok(())
    }

    async fn handoff_message(
        &mut self,
        message: &MessageRecord,
        mark: SyncTime,
        now: SyncTime,
    ) -> Result<(), InboxStoreError> {
        match self.handoff.handoff(message).await {
            Ok(answer) => self.store.record_handoff(&message.message_id, answer, mark, now).await,
            Err(error) => {
                tracing::warn!(%error, "inbox: session handoff is still pending");
                Ok(())
            }
        }
    }

    pub(super) async fn earliest_due(
        &mut self,
        next_mark: SyncTime,
    ) -> Result<Option<SyncTime>, InboxStoreError> {
        self.store.earliest_due(next_mark).await
    }

    pub(super) async fn acks_due(&mut self, mark: SyncTime) -> Result<Vec<PendingAck>, InboxStoreError> {
        self.store.acks_due(mark).await
    }

    pub(super) async fn mark_ack_sent(
        &mut self,
        ack: &PendingAck,
        now: SyncTime,
    ) -> Result<(), InboxStoreError> {
        self.store.mark_ack_sent(&ack.message_id, now).await
    }

    pub(super) async fn prune_ended(&mut self, now: SyncTime) -> Result<(), InboxStoreError> {
        self.store.prune_ended(now).await
    }
}
