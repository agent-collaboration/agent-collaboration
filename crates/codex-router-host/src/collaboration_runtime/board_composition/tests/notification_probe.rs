//! Observe actual consumer submissions and pause an actual receiver commit.
use board_sync_protocol::BoardDeliveryTypes;
use fleet_sync_protocol::{
    DeliveryEnvelope, DeliveryItem, Epoch, ItemId, ReplicatedTransaction, RowChange,
};
use fleet_sync_receiver::{AppliedPiece, HookError};
use std::io;
use tokio::sync::{Mutex, Notify, oneshot};

struct PieceGate {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}
impl PieceGate {
    async fn wait_for_release(self) -> Result<(), HookError> {
        let failed = || {
            HookError::Storage(fleet_sync_receiver::StoreError::Unavailable {
                message: "native receiver piece gate closed".to_owned(),
            })
        };
        self.entered.send(()).map_err(|_| failed())?;
        self.release.await.map_err(|_| failed())
    }
}

#[derive(Default)]
pub(in crate::collaboration_runtime::board_composition) struct NotificationProbe {
    piece: Mutex<Option<PieceGate>>,
    frontier: Mutex<Option<PieceGate>>,
    submissions: Mutex<Vec<(ItemId, Option<Epoch>)>>,
    envelopes: Mutex<Vec<(DeliveryEnvelope<BoardDeliveryTypes>, Vec<u8>)>>,
    submitted: Notify,
}

impl NotificationProbe {
    pub async fn hold_next_piece(
        &self,
    ) -> io::Result<(oneshot::Receiver<()>, oneshot::Sender<()>)> {
        let (entered, observed) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let mut gate = self.piece.lock().await;
        if gate.is_some() {
            return Err(io::Error::other("a native receiver piece is already held"));
        }
        *gate = Some(PieceGate {
            entered,
            release: released,
        });
        Ok((observed, release))
    }

    pub async fn hold_next_frontier(
        &self,
    ) -> io::Result<(oneshot::Receiver<()>, oneshot::Sender<()>)> {
        let (entered, observed) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let mut gate = self.frontier.lock().await;
        if gate.is_some() {
            return Err(io::Error::other("a native frontier commit is already held"));
        }
        *gate = Some(PieceGate {
            entered,
            release: released,
        });
        Ok((observed, release))
    }

    pub async fn applied(
        &self,
        piece: &AppliedPiece<'_, board_sync_protocol::BoardAnnotation>,
    ) -> Result<(), HookError> {
        if piece
            .transactions
            .iter()
            .any(|transaction| {
                match transaction {
            ReplicatedTransaction::Rows { rows, .. } => rows.iter().any(|row|
                matches!(row, RowChange::Upsert { table, .. } if table == "handoff_frontier")),
            ReplicatedTransaction::SchemaStep { .. } => false,
        }
            })
            && let Some(gate) = self.frontier.lock().await.take()
        {
            gate.wait_for_release().await?;
        }
        if let Some(gate) = self.piece.lock().await.take() {
            gate.wait_for_release().await?;
        }
        Ok(())
    }

    pub async fn record_submission(
        &self,
        item: &fleet_sync_protocol::DeliveryEnvelope<board_sync_protocol::BoardDeliveryTypes>,
        epoch: Option<Epoch>,
    ) -> Result<(), serde_json::Error> {
        let encoded = serde_json::to_vec(item)?;
        let observed = serde_json::from_slice(&encoded)?;
        self.envelopes.lock().await.push((observed, encoded));
        self.submissions.lock().await.push((item.id(), epoch));
        self.submitted.notify_waiters();
        Ok(())
    }

    pub async fn submitted_envelopes(&self, item: ItemId) -> Vec<Vec<u8>> {
        self.envelopes
            .lock()
            .await
            .iter()
            .filter(|(envelope, _)| envelope.id() == item)
            .map(|(_, encoded)| encoded.clone())
            .collect()
    }

    pub async fn submitted_notification_envelopes(
        &self,
        frozen: &DeliveryItem<BoardDeliveryTypes>,
    ) -> Vec<(ItemId, Vec<u8>)> {
        self.envelopes
            .lock()
            .await
            .iter()
            .filter_map(|(envelope, encoded)| match envelope {
                DeliveryEnvelope::Item(item)
                    if item.sender() == frozen.sender()
                        && item.recipient() == frozen.recipient()
                        && item.body() == frozen.body() =>
                {
                    Some((item.id(), encoded.clone()))
                }
                _ => None,
            })
            .collect()
    }

    pub async fn wait_submission_count(&self, item: ItemId, count: usize) -> Vec<Vec<u8>> {
        loop {
            let changed = self.submitted.notified();
            let envelopes = self.submitted_envelopes(item).await;
            if envelopes.len() >= count {
                return envelopes;
            }
            changed.await;
        }
    }

    pub async fn wait_submission(&self, item: ItemId) -> Option<Epoch> {
        loop {
            let changed = self.submitted.notified();
            if let Some((_, epoch)) = self
                .submissions
                .lock()
                .await
                .iter()
                .find(|(id, _)| *id == item)
            {
                return *epoch;
            }
            changed.await;
        }
    }

    pub async fn has_submission(&self, item: ItemId) -> bool {
        self.submissions
            .lock()
            .await
            .iter()
            .any(|(id, _)| *id == item)
    }
}
