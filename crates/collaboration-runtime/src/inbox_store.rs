//! Durable delivery custody in machine_local.sqlite.
use crate::{MachineLocalStore, machine_local_store::MachineLocalWrite};
use runtime_primitives::SyncTime;
use serde::{Deserialize, Serialize};
use sqlite_storage_maintenance::TransactionStart;
use sqlx::SqliteConnection;
use workspace_model::{
    MessageId,
    central_mail::{DeliveryAnswer, MessageRecord},
};

const RETENTION_MILLIS: i64 = 24 * 60 * 60 * 1_000;

#[derive(Debug, thiserror::Error)]
pub(crate) enum InboxStoreError {
    #[error("inbox storage unavailable")]
    Database(#[from] sqlx::Error),
    #[error("inbox state could not be encoded")]
    Encode(#[from] serde_json::Error),
    #[error("inbox message {message_id} has invalid stored {field}")]
    Corrupt {
        message_id: String,
        field: &'static str,
    },
    #[error("inbox message {message_id} was not {expected}")]
    UnexpectedState {
        message_id: MessageId,
        expected: &'static str,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum RecordDelivery {
    Inserted,
    Duplicate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PendingAck {
    pub(crate) message_id: MessageId,
    pub(crate) answer: DeliveryAnswer,
    pub(crate) flush_at: SyncTime,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum InboxRowState {
    PendingHandoff,
    AckPending {
        answer: DeliveryAnswer,
        flush_at_ms: i64,
    },
    AckSent {
        answer: DeliveryAnswer,
    },
}

enum StoredDelivery {
    PendingHandoff(Box<MessageRecord>),
    AckPending(PendingAck),
    AckSent { answer: DeliveryAnswer },
}

struct InboxRow {
    message_id: String,
    message_json: Option<String>,
    delivery_state_json: String,
}

impl InboxRow {
    fn decode(self) -> Result<StoredDelivery, InboxStoreError> {
        let corrupt = |field| InboxStoreError::Corrupt {
            message_id: self.message_id.clone(),
            field,
        };
        let message_id: MessageId = self.message_id.parse().map_err(|_| corrupt("message_id"))?;
        let state = serde_json::from_str(&self.delivery_state_json)
            .map_err(|_| corrupt("delivery_state_json"))?;
        match (state, self.message_json.as_deref()) {
            (InboxRowState::PendingHandoff, Some(json)) => {
                let message: MessageRecord =
                    serde_json::from_str(json).map_err(|_| corrupt("message_json"))?;
                if message.message_id != message_id {
                    return Err(corrupt("message_json.message_id"));
                }
                Ok(StoredDelivery::PendingHandoff(Box::new(message)))
            }
            (
                InboxRowState::AckPending {
                    answer,
                    flush_at_ms,
                },
                None,
            ) => {
                let flush_at = SyncTime::from_unix_millis(flush_at_ms);
                if central_link_client::mark_in_window(flush_at) != Some(flush_at) {
                    return Err(corrupt("delivery_state_json.flush_at_ms"));
                }
                Ok(StoredDelivery::AckPending(PendingAck {
                    message_id,
                    answer,
                    flush_at,
                }))
            }
            (InboxRowState::AckSent { answer }, None) => Ok(StoredDelivery::AckSent { answer }),
            (InboxRowState::PendingHandoff, None)
            | (InboxRowState::AckPending { .. } | InboxRowState::AckSent { .. }, Some(_)) => {
                Err(corrupt("message_json"))
            }
        }
    }
}

pub(crate) struct InboxStore {
    store: MachineLocalStore,
}

impl InboxStore {
    pub(crate) const fn new(store: MachineLocalStore) -> Self {
        Self { store }
    }

    pub(crate) async fn record_delivery(
        &mut self,
        message: &MessageRecord,
        mark: SyncTime,
        now: SyncTime,
    ) -> Result<RecordDelivery, InboxStoreError> {
        let message_id = message.message_id.as_str();
        let mut txn = self.store.transaction(TransactionStart::Immediate).await?;
        match read_delivery(&mut txn, &message.message_id).await? {
            None => {
                let payload = serde_json::to_string(message)?;
                let state = serde_json::to_string(&InboxRowState::PendingHandoff)?;
                sqlx::query(
                    "INSERT INTO inbox_messages
                     (message_id, message_json, delivery_state_json, created_at, updated_at)
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind(message_id)
                .bind(payload)
                .bind(state)
                .bind(now.unix_millis())
                .bind(now.unix_millis())
                .execute(&mut *txn)
                .await?;
                txn.commit().await?;
                Ok(RecordDelivery::Inserted)
            }
            Some(delivery) => {
                if let StoredDelivery::AckSent { answer } = delivery {
                    set_ack_state(
                        &mut txn,
                        &message.message_id,
                        ack_pending(answer, mark),
                        now,
                    )
                    .await?;
                }
                txn.commit().await?;
                Ok(RecordDelivery::Duplicate)
            }
        }
    }

    pub(crate) async fn record_handoff(
        &mut self,
        message_id: &MessageId,
        answer: DeliveryAnswer,
        mark: SyncTime,
        now: SyncTime,
    ) -> Result<(), InboxStoreError> {
        let mut txn = self.store.transaction(TransactionStart::Immediate).await?;
        let Some(StoredDelivery::PendingHandoff(_)) = read_delivery(&mut txn, message_id).await?
        else {
            return Err(unexpected(message_id, "pending handoff"));
        };
        set_ack_state(&mut txn, message_id, ack_pending(answer, mark), now).await?;
        txn.commit().await?;
        Ok(())
    }

    pub(crate) async fn mark_ack_sent(
        &mut self,
        message_id: &MessageId,
        now: SyncTime,
    ) -> Result<(), InboxStoreError> {
        let mut txn = self.store.transaction(TransactionStart::Immediate).await?;
        let Some(StoredDelivery::AckPending(ack)) = read_delivery(&mut txn, message_id).await?
        else {
            return Err(unexpected(message_id, "pending Ack"));
        };
        set_ack_state(
            &mut txn,
            message_id,
            InboxRowState::AckSent { answer: ack.answer },
            now,
        )
        .await?;
        txn.commit().await?;
        Ok(())
    }

    pub(crate) async fn pending_handoffs(&mut self) -> Result<Vec<MessageRecord>, InboxStoreError> {
        let mut handoffs = Vec::new();
        for delivery in self.active_deliveries().await? {
            if let StoredDelivery::PendingHandoff(message) = delivery {
                handoffs.push(*message);
            }
        }
        Ok(handoffs)
    }

    pub(crate) async fn pending_acks(&mut self) -> Result<Vec<PendingAck>, InboxStoreError> {
        let mut acks = Vec::new();
        for delivery in self.active_deliveries().await? {
            if let StoredDelivery::AckPending(ack) = delivery {
                acks.push(ack);
            }
        }
        Ok(acks)
    }

    pub(crate) async fn acks_due(
        &mut self,
        mark: SyncTime,
    ) -> Result<Vec<PendingAck>, InboxStoreError> {
        Ok(self
            .pending_acks()
            .await?
            .into_iter()
            .filter(|ack| ack.flush_at <= mark)
            .collect())
    }

    pub(crate) async fn earliest_due(
        &mut self,
        next_mark: SyncTime,
    ) -> Result<Option<SyncTime>, InboxStoreError> {
        let active_due = self
            .active_deliveries()
            .await?
            .into_iter()
            .filter_map(|delivery| match delivery {
                StoredDelivery::PendingHandoff(_) => Some(next_mark),
                StoredDelivery::AckPending(ack) => Some(ack.flush_at),
                StoredDelivery::AckSent { .. } => None,
            })
            .min();
        let oldest_sent = sqlx::query_scalar!(
            "SELECT min(updated_at) AS \"oldest?: i64\" FROM inbox_messages
             WHERE json_extract(delivery_state_json, '$.kind') = 'ackSent'",
        )
        .fetch_one(&mut self.store.connection)
        .await?;
        let prune_due = oldest_sent.map(|ended| {
            central_link_client::next_mark_after(SyncTime::from_unix_millis(
                ended.saturating_add(RETENTION_MILLIS),
            ))
        });
        Ok(active_due.into_iter().chain(prune_due).min())
    }

    pub(crate) async fn prune_ended(&mut self, now: SyncTime) -> Result<(), InboxStoreError> {
        sqlx::query(
            "DELETE FROM inbox_messages WHERE updated_at < ?
             AND json_extract(delivery_state_json, '$.kind') = 'ackSent'",
        )
        .bind(now.unix_millis().saturating_sub(RETENTION_MILLIS))
        .execute_as_transaction(&mut self.store)
        .await?;
        Ok(())
    }

    async fn active_deliveries(&mut self) -> Result<Vec<StoredDelivery>, InboxStoreError> {
        sqlx::query_as!(
            InboxRow,
            "SELECT message_id, message_json, delivery_state_json FROM inbox_messages
             WHERE json_extract(delivery_state_json, '$.kind') IS NOT 'ackSent'
             ORDER BY created_at, message_id",
        )
        .fetch_all(&mut self.store.connection)
        .await?
        .into_iter()
        .map(InboxRow::decode)
        .collect()
    }
}

async fn read_delivery(
    connection: &mut SqliteConnection,
    message_id: &MessageId,
) -> Result<Option<StoredDelivery>, InboxStoreError> {
    let key = message_id.as_str();
    sqlx::query_as!(
        InboxRow,
        "SELECT message_id, message_json, delivery_state_json FROM inbox_messages WHERE message_id = ?",
        key,
    )
    .fetch_optional(connection)
    .await?
    .map(InboxRow::decode)
    .transpose()
}

fn ack_pending(answer: DeliveryAnswer, mark: SyncTime) -> InboxRowState {
    InboxRowState::AckPending {
        answer,
        flush_at_ms: mark.unix_millis(),
    }
}

async fn set_ack_state(
    connection: &mut SqliteConnection,
    message_id: &MessageId,
    state: InboxRowState,
    now: SyncTime,
) -> Result<(), InboxStoreError> {
    sqlx::query(
        "UPDATE inbox_messages SET message_json = NULL, delivery_state_json = ?, updated_at = ?
         WHERE message_id = ?",
    )
    .bind(serde_json::to_string(&state)?)
    .bind(now.unix_millis())
    .bind(message_id.as_str())
    .execute(connection)
    .await?;
    Ok(())
}

fn unexpected(message_id: &MessageId, expected: &'static str) -> InboxStoreError {
    InboxStoreError::UnexpectedState {
        message_id: message_id.clone(),
        expected,
    }
}

#[cfg(test)]
#[path = "inbox_store_test.rs"]
mod tests;
