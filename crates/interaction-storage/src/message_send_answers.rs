//! Recorded RPC answers survive push pruning; library custody alone decides replay and conflict.
use crate::{InteractionStore, StorageError, push_record_rows};
use chrono::{DateTime, Utc};
use collaboration_protocol::{MailItemId, MessageSendResult, RouterLink};
use durable_steps::{Host, storage::TransactionScope, turso_store::TursoScope};
use sqlx::Transaction;
use sqlx_turso::{Turso, TursoConnection};

// session_identity renders at most 120 display-name characters, or the 64-byte
// endpoint and eight session characters, plus its emoji. This leaves headroom
// over every output of that existing formatter without narrowing its inputs.
const MAX_TARGET_IDENTITY_BYTES: usize = 1024;

#[derive(Clone, Debug)]
pub struct MessageSendAnswer {
    pub item_id: MailItemId,
    pub target_identity: String,
    pub result: Option<MessageSendResult>,
    pub recorded_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct MessageSendAnswerRow {
    item_id: String,
    target_identity: String,
    result: Option<String>,
    recorded_at: String,
    settled_at_ms: Option<i64>,
    settled_at_storage: String,
}

fn validate_target_identity(identity: &str) -> Result<(), StorageError> {
    if identity.trim().is_empty() || identity.len() > MAX_TARGET_IDENTITY_BYTES {
        return Err(StorageError::InvalidRecord);
    }
    Ok(())
}

fn validate_result(
    item: &MailItemId,
    target_identity: &str,
    result: &MessageSendResult,
) -> Result<(), StorageError> {
    let result = match result {
        MessageSendResult::Local(result) => result,
        MessageSendResult::Queued { queued } => {
            return if &queued.item == item && queued.target_identity == target_identity {
                Ok(())
            } else {
                Err(StorageError::InvalidRecord)
            };
        }
    };
    let link = RouterLink::parse(&result.link).map_err(|_| StorageError::InvalidRecord)?;
    if result.push_id.as_str() != item.as_str()
        || result.target_identity != target_identity
        || link.push_id() != &result.push_id
        || link.machine_id().as_str() != String::from(result.target.endpoint.service_id.clone())
    {
        return Err(StorageError::InvalidRecord);
    }
    Ok(())
}

impl MessageSendAnswerRow {
    fn decode(self) -> Result<MessageSendAnswer, StorageError> {
        let item_id = self
            .item_id
            .try_into()
            .map_err(|_| StorageError::InvalidStoredRecord)?;
        validate_target_identity(&self.target_identity)
            .map_err(|_| StorageError::InvalidStoredRecord)?;
        let result: Option<MessageSendResult> = self
            .result
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| StorageError::InvalidStoredRecord)?;
        if let Some(result) = &result {
            validate_result(&item_id, &self.target_identity, result)
                .map_err(|_| StorageError::InvalidStoredRecord)?;
        }
        if result.is_some() != self.settled_at_ms.is_some()
            || (self.settled_at_ms.is_some() && self.settled_at_storage != "integer")
            || (self.settled_at_ms.is_none() && self.settled_at_storage != "null")
        {
            return Err(StorageError::InvalidStoredRecord);
        }
        let settled_at = match self.settled_at_ms {
            Some(value) => Some(
                DateTime::from_timestamp_millis(value).ok_or(StorageError::InvalidStoredRecord)?,
            ),
            None => None,
        };
        Ok(MessageSendAnswer {
            item_id,
            target_identity: self.target_identity,
            result,
            recorded_at: push_record_rows::decode_timestamp(self.recorded_at)
                .map_err(|_| StorageError::InvalidStoredRecord)?,
            settled_at,
        })
    }
}

async fn read_answer(
    connection: &mut TursoConnection,
    item: &MailItemId,
) -> Result<Option<MessageSendAnswer>, StorageError> {
    sqlx::query_as::<sqlx_turso::Turso, MessageSendAnswerRow>(
        "SELECT item_id,target_identity,result,recorded_at,settled_at_ms,typeof(settled_at_ms) AS settled_at_storage FROM message_send_answers WHERE item_id=?",
    )
        .bind(item.as_str())
        .fetch_optional(connection).await?
        .map(MessageSendAnswerRow::decode).transpose()
}

pub(crate) async fn prune_settled_message_send_answers(
    transaction: &mut Transaction<'_, Turso>,
    cutoff_ms: i64,
    batch_size: i64,
) -> Result<u64, StorageError> {
    let candidates = sqlx::query_as::<sqlx_turso::Turso, MessageSendAnswerRow>(
        "SELECT item_id,target_identity,result,recorded_at,settled_at_ms,typeof(settled_at_ms) AS settled_at_storage FROM message_send_answers WHERE settled_at_ms<? AND NOT EXISTS (SELECT 1 FROM router_pushes p WHERE p.push_id=message_send_answers.item_id AND p.delivery_state IN ('pending','held','attempted')) AND NOT EXISTS (SELECT 1 FROM local_delivery_retries r WHERE r.item_id=message_send_answers.item_id AND r.scheduled_retry_at IS NOT NULL) AND NOT EXISTS (SELECT 1 FROM pending_poll_mail poll WHERE poll.item_id=message_send_answers.item_id) AND NOT EXISTS (SELECT 1 FROM local_cancel_deliveries c WHERE (c.item_id=message_send_answers.item_id OR c.target_item=message_send_answers.item_id) AND c.outcome_json IS NULL) ORDER BY settled_at_ms,item_id LIMIT ?",
    )
        .bind(cutoff_ms)
        .bind(batch_size)
        .fetch_all(&mut **transaction).await?;
    let mut pruned = 0;
    for row in candidates {
        let answer = row.decode()?;
        if answer.result.is_none() || answer.settled_at.is_none() {
            return Err(StorageError::InvalidStoredRecord);
        }
        pruned += sqlx::query::<sqlx_turso::Turso>(
            "DELETE FROM message_send_answers WHERE item_id=? AND settled_at_ms<?",
        )
        .bind(answer.item_id.as_str())
        .bind(cutoff_ms)
        .execute(&mut **transaction)
        .await?
        .rows_affected();
    }
    Ok(pruned)
}

impl InteractionStore {
    /// Read only after library custody has established the caller's same-item replay.
    pub async fn message_send_answer(
        &self,
        item: &MailItemId,
    ) -> Result<Option<MessageSendAnswer>, StorageError> {
        read_answer(&mut *self.writer.lock().await, item).await
    }
}

pub async fn read_message_send_answer_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: &MailItemId,
) -> Result<Option<MessageSendAnswer>, StorageError> {
    read_answer(scope.executor(), item).await
}

/// Insert only with a newly admitted library item, in its admission transaction.
pub async fn record_message_send_admission<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: &MailItemId,
    target_identity: &str,
    recorded_at: DateTime<Utc>,
) -> Result<(), StorageError> {
    validate_target_identity(target_identity)?;
    let recorded_at = push_record_rows::serialize_timestamp(recorded_at);
    sqlx_turso::query!(
        "INSERT INTO message_send_answers(item_id,target_identity,result,recorded_at) VALUES (?,?,NULL,?)",
        item.as_str(),target_identity,recorded_at)
        .execute(scope.executor()).await?;
    Ok(())
}

/// Admission or an outcome hook records the complete answer in its owning transaction.
pub async fn record_message_send_outcome<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: &MailItemId,
    result: &MessageSendResult,
    now_ms: i64,
) -> Result<(), StorageError> {
    let admission = read_answer(scope.executor(), item)
        .await?
        .ok_or(StorageError::InvalidRecord)?;
    validate_result(item, &admission.target_identity, result)?;
    if let Some(existing) = &admission.result {
        let existing =
            serde_json::to_value(existing).map_err(|_| StorageError::InvalidStoredRecord)?;
        let incoming = serde_json::to_value(result).map_err(|_| StorageError::InvalidRecord)?;
        if existing == incoming {
            return Ok(());
        }
    }
    let encoded = push_record_rows::serialize_json(result)?;
    let changed = sqlx::query::<sqlx_turso::Turso>(
        "UPDATE message_send_answers SET result=?,settled_at_ms=? WHERE item_id=?",
    )
    .bind(encoded)
    .bind(now_ms)
    .bind(item.as_str())
    .execute(scope.executor())
    .await?;
    if changed.rows_affected() != 1 {
        return Err(StorageError::InvalidRecord);
    }
    Ok(())
}
