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
#[serde(deny_unknown_fields)]
struct InboxRowState {
    answer: DeliveryAnswer,
    ack: AckSchedule,
    ended_at_ms: Option<i64>,
}

/// Handoff and socket progress are independent. Replays retain the fact
/// that the first Ack went out, including when a handoff finishes mid-replay.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum AckSchedule {
    First { flush_at_ms: i64 },
    Replay { flush_at_ms: i64 },
    Sent,
}

impl AckSchedule {
    fn mark(&self) -> Option<SyncTime> {
        match *self {
            Self::First { flush_at_ms } | Self::Replay { flush_at_ms } => {
                Some(SyncTime::from_unix_millis(flush_at_ms))
            }
            Self::Sent => None,
        }
    }

    fn was_sent(&self) -> bool {
        !matches!(self, Self::First { .. })
    }

    fn reoffer(&mut self, mark: SyncTime) -> bool {
        if matches!(self, Self::Sent) {
            *self = Self::Replay {
                flush_at_ms: mark.unix_millis(),
            };
            true
        } else {
            false
        }
    }
}

struct StoredDelivery {
    message_id: MessageId,
    message: Option<MessageRecord>,
    state: InboxRowState,
}

impl StoredDelivery {
    fn pending_ack(self) -> Option<PendingAck> {
        Some(PendingAck {
            message_id: self.message_id,
            answer: self.state.answer,
            flush_at: self.state.ack.mark()?,
        })
    }

    fn end_if_complete(&mut self, now: SyncTime) {
        if self.message.is_none() && self.state.ack.was_sent() {
            self.state.ended_at_ms.get_or_insert(now.unix_millis());
        }
    }
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
        let state: InboxRowState = serde_json::from_str(&self.delivery_state_json)
            .map_err(|_| corrupt("delivery_state_json"))?;
        let message: Option<MessageRecord> = self
            .message_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| corrupt("message_json"))?;
        if let Some(message) = &message {
            if message.message_id != message_id {
                return Err(corrupt("message_json.message_id"));
            }
            if state.answer != DeliveryAnswer::Accepted {
                return Err(corrupt("message_json"));
            }
        }
        if let Some(mark) = state.ack.mark()
            && central_link_client::mark_in_window(mark) != Some(mark)
        {
            return Err(corrupt("delivery_state_json.ack.flush_at_ms"));
        }
        if state.ended_at_ms.is_some() != (message.is_none() && state.ack.was_sent()) {
            return Err(corrupt("delivery_state_json.ended_at_ms"));
        }
        Ok(StoredDelivery {
            message_id,
            message,
            state,
        })
    }
}

pub(crate) struct InboxStore {
    store: MachineLocalStore,
}

impl InboxStore {
    pub(crate) const fn new(store: MachineLocalStore) -> Self {
        Self { store }
    }

    /// A repeat uses the first stored answer even when lookup is unavailable.
    pub(crate) async fn reoffer_duplicate(
        &mut self,
        message_id: &MessageId,
        mark: SyncTime,
        now: SyncTime,
    ) -> Result<bool, InboxStoreError> {
        let mut transaction = self.store.transaction(TransactionStart::Immediate).await?;
        let Some(mut delivery) = read_delivery(&mut transaction, message_id).await? else {
            transaction.commit().await?;
            return Ok(false);
        };
        if delivery.state.ack.reoffer(mark) {
            update_delivery(&mut transaction, &delivery, now).await?;
        }
        transaction.commit().await?;
        Ok(true)
    }

    pub(crate) async fn record_delivery(
        &mut self,
        message: &MessageRecord,
        answer: DeliveryAnswer,
        mark: SyncTime,
        now: SyncTime,
    ) -> Result<RecordDelivery, InboxStoreError> {
        let mut transaction = self.store.transaction(TransactionStart::Immediate).await?;
        let recorded = match read_delivery(&mut transaction, &message.message_id).await? {
            None => {
                let payload = (answer == DeliveryAnswer::Accepted)
                    .then(|| serde_json::to_string(message))
                    .transpose()?;
                let state = serde_json::to_string(&InboxRowState {
                    answer,
                    ack: AckSchedule::First {
                        flush_at_ms: mark.unix_millis(),
                    },
                    ended_at_ms: None,
                })?;
                sqlx::query(
                    "INSERT INTO inbox_messages
                     (message_id, message_json, delivery_state_json, created_at, updated_at)
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind(message.message_id.as_str())
                .bind(payload)
                .bind(state)
                .bind(now.unix_millis())
                .bind(now.unix_millis())
                .execute(&mut *transaction)
                .await?;
                RecordDelivery::Inserted
            }
            Some(mut delivery) => {
                if delivery.state.ack.reoffer(mark) {
                    update_delivery(&mut transaction, &delivery, now).await?;
                }
                RecordDelivery::Duplicate
            }
        };
        transaction.commit().await?;
        Ok(recorded)
    }

    pub(crate) async fn record_handoff(
        &mut self,
        message_id: &MessageId,
        now: SyncTime,
    ) -> Result<(), InboxStoreError> {
        let mut transaction = self.store.transaction(TransactionStart::Immediate).await?;
        let mut delivery = read_delivery(&mut transaction, message_id)
            .await?
            .ok_or_else(|| unexpected(message_id, "pending handoff"))?;
        if delivery.message.take().is_none() {
            return Err(unexpected(message_id, "pending handoff"));
        }
        delivery.end_if_complete(now);
        update_delivery(&mut transaction, &delivery, now).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub(crate) async fn mark_ack_sent(
        &mut self,
        message_id: &MessageId,
        now: SyncTime,
    ) -> Result<(), InboxStoreError> {
        let mut transaction = self.store.transaction(TransactionStart::Immediate).await?;
        let mut delivery = read_delivery(&mut transaction, message_id)
            .await?
            .ok_or_else(|| unexpected(message_id, "pending Ack"))?;
        if delivery.state.ack.mark().is_none() {
            return Err(unexpected(message_id, "pending Ack"));
        }
        delivery.state.ack = AckSchedule::Sent;
        delivery.end_if_complete(now);
        update_delivery(&mut transaction, &delivery, now).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub(crate) async fn reoffer_recent_acks(
        &mut self,
        mark: SyncTime,
        now: SyncTime,
    ) -> Result<(), InboxStoreError> {
        let cutoff = now.unix_millis().saturating_sub(RETENTION_MILLIS);
        let mut transaction = self.store.transaction(TransactionStart::Immediate).await?;
        let rows = sqlx::query_as!(
            InboxRow,
            "SELECT message_id, message_json, delivery_state_json FROM inbox_messages
             WHERE created_at >= ? AND json_extract(delivery_state_json, '$.ack.kind') = 'sent'
             ORDER BY created_at, message_id",
            cutoff
        )
        .fetch_all(&mut *transaction)
        .await?;
        for row in rows {
            let mut delivery = row.decode()?;
            if delivery.state.answer == DeliveryAnswer::Accepted && delivery.state.ack.reoffer(mark)
            {
                update_delivery(&mut transaction, &delivery, now).await?;
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    pub(crate) async fn pending_handoffs(&mut self) -> Result<Vec<MessageRecord>, InboxStoreError> {
        let rows = sqlx::query_as!(
            InboxRow,
            "SELECT message_id, message_json, delivery_state_json FROM inbox_messages
             WHERE message_json IS NOT NULL ORDER BY created_at, message_id",
        )
        .fetch_all(&mut self.store.connection)
        .await?;
        Ok(rows
            .into_iter()
            .map(InboxRow::decode)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(|delivery| delivery.message)
            .collect())
    }

    pub(crate) async fn pending_acks(&mut self) -> Result<Vec<PendingAck>, InboxStoreError> {
        let rows = sqlx::query_as!(
            InboxRow,
            "SELECT message_id, message_json, delivery_state_json FROM inbox_messages
             WHERE json_extract(delivery_state_json, '$.ack.kind') IS NOT 'sent'
             ORDER BY created_at, message_id",
        )
        .fetch_all(&mut self.store.connection)
        .await?;
        Ok(rows
            .into_iter()
            .map(InboxRow::decode)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(StoredDelivery::pending_ack)
            .collect())
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

    pub(crate) async fn next_prune(&mut self) -> Result<Option<SyncTime>, InboxStoreError> {
        let oldest = sqlx::query_scalar!(
            "SELECT min(json_extract(delivery_state_json, '$.ended_at_ms')) AS \"oldest?: i64\"
             FROM inbox_messages",
        )
        .fetch_one(&mut self.store.connection)
        .await?;
        Ok(oldest.map(|ended| {
            central_link_client::next_mark_after(SyncTime::from_unix_millis(
                ended.saturating_add(RETENTION_MILLIS),
            ))
        }))
    }

    pub(crate) async fn prune_ended(&mut self, now: SyncTime) -> Result<(), InboxStoreError> {
        sqlx::query(
            "DELETE FROM inbox_messages WHERE json_extract(delivery_state_json, '$.ended_at_ms') < ?",
        )
        .bind(now.unix_millis().saturating_sub(RETENTION_MILLIS))
        .execute_as_transaction(&mut self.store)
        .await?;
        Ok(())
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

async fn update_delivery(
    connection: &mut SqliteConnection,
    delivery: &StoredDelivery,
    now: SyncTime,
) -> Result<(), InboxStoreError> {
    sqlx::query(
        "UPDATE inbox_messages SET message_json = CASE WHEN ? THEN NULL ELSE message_json END,
            delivery_state_json = ?, updated_at = ? WHERE message_id = ?",
    )
    .bind(delivery.message.is_none())
    .bind(serde_json::to_string(&delivery.state)?)
    .bind(now.unix_millis())
    .bind(delivery.message_id.as_str())
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
