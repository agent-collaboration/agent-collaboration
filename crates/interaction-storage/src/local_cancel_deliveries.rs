//! Local attribution accompanies SDK custody; it never decides replay or conflict.
use crate::{InteractionStore, StorageError, push_record_rows};
use chrono::{DateTime, Utc};
use collaboration_mail_protocol::{CancelCause, CarrierAcceptance, MailItemId};
use durable_steps::{Host, storage::TransactionScope, turso_store::TursoScope};
use fleet_sync_protocol::RecipientOutcome;
use sqlx::Transaction;
use sqlx_turso::{Turso, TursoConnection};

#[derive(Clone, Debug)]
pub struct LocalCancelDelivery {
    pub item_id: MailItemId,
    pub target_item: MailItemId,
    pub attribution: CancelCause,
    pub outcome: Option<RecipientOutcome<CarrierAcceptance>>,
    pub recorded_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalCancelPreparation {
    Inserted,
    Kept,
}

#[derive(sqlx::FromRow)]
struct LocalCancelRow {
    item_id: String,
    target_item: String,
    attribution_json: String,
    outcome_json: Option<String>,
    recorded_at: String,
    settled_at_ms: Option<i64>,
    settled_at_storage: String,
}

fn validate_outcome(
    item: &MailItemId,
    outcome: &RecipientOutcome<CarrierAcceptance>,
) -> Result<(), StorageError> {
    if let RecipientOutcome::Prevented { cancel } | RecipientOutcome::TooLate { cancel, .. } =
        outcome
        && cancel.as_uuid().to_string() != item.as_str()
    {
        return Err(StorageError::InvalidRecord);
    }
    Ok(())
}

impl LocalCancelRow {
    fn decode(self) -> Result<LocalCancelDelivery, StorageError> {
        let item_id = self
            .item_id
            .try_into()
            .map_err(|_| StorageError::InvalidStoredRecord)?;
        let target_item = self
            .target_item
            .try_into()
            .map_err(|_| StorageError::InvalidStoredRecord)?;
        let attribution = serde_json::from_str(&self.attribution_json)
            .map_err(|_| StorageError::InvalidStoredRecord)?;
        // The pinned SDK's Serde implementation owns the private, lossless codec.
        let outcome = self
            .outcome_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| StorageError::InvalidStoredRecord)?;
        if let Some(outcome) = &outcome {
            validate_outcome(&item_id, outcome).map_err(|_| StorageError::InvalidStoredRecord)?;
        }
        if outcome.is_some() != self.settled_at_ms.is_some()
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
        Ok(LocalCancelDelivery {
            item_id,
            target_item,
            attribution,
            outcome,
            recorded_at: push_record_rows::decode_timestamp(self.recorded_at)
                .map_err(|_| StorageError::InvalidStoredRecord)?,
            settled_at,
        })
    }
}

async fn read_cancel(
    connection: &mut TursoConnection,
    item: &MailItemId,
) -> Result<Option<LocalCancelDelivery>, StorageError> {
    sqlx::query_as::<sqlx_turso::Turso, LocalCancelRow>(
        "SELECT item_id,target_item,attribution_json,outcome_json,recorded_at,settled_at_ms,typeof(settled_at_ms) AS settled_at_storage FROM local_cancel_deliveries WHERE item_id=?",
    )
        .bind(item.as_str())
        .fetch_optional(connection).await?
        .map(LocalCancelRow::decode).transpose()
}

pub(crate) async fn prune_settled_local_cancels(
    transaction: &mut Transaction<'_, Turso>,
    cutoff_ms: i64,
    batch_size: i64,
) -> Result<u64, StorageError> {
    let candidates = sqlx::query_as::<sqlx_turso::Turso, LocalCancelRow>(
        "SELECT item_id,target_item,attribution_json,outcome_json,recorded_at,settled_at_ms,typeof(settled_at_ms) AS settled_at_storage FROM local_cancel_deliveries WHERE settled_at_ms<? AND NOT EXISTS (SELECT 1 FROM router_pushes p WHERE p.push_id IN (local_cancel_deliveries.item_id,local_cancel_deliveries.target_item) AND p.delivery_state IN ('pending','held','attempted')) AND NOT EXISTS (SELECT 1 FROM local_delivery_retries r WHERE r.item_id IN (local_cancel_deliveries.item_id,local_cancel_deliveries.target_item) AND r.scheduled_retry_at IS NOT NULL) AND NOT EXISTS (SELECT 1 FROM pending_poll_mail poll WHERE poll.item_id IN (local_cancel_deliveries.item_id,local_cancel_deliveries.target_item)) AND NOT EXISTS (SELECT 1 FROM message_send_answers answer WHERE answer.item_id=local_cancel_deliveries.target_item AND answer.result IS NULL) ORDER BY settled_at_ms,item_id LIMIT ?",
    )
        .bind(cutoff_ms)
        .bind(batch_size)
        .fetch_all(&mut **transaction).await?;
    let mut pruned = 0;
    for row in candidates {
        let cancel = row.decode()?;
        if cancel.outcome.is_none() || cancel.settled_at.is_none() {
            return Err(StorageError::InvalidStoredRecord);
        }
        pruned += sqlx::query::<sqlx_turso::Turso>(
            "DELETE FROM local_cancel_deliveries WHERE item_id=? AND settled_at_ms<?",
        )
        .bind(cancel.item_id.as_str())
        .bind(cutoff_ms)
        .execute(&mut **transaction)
        .await?
        .rows_affected();
    }
    Ok(pruned)
}

impl InteractionStore {
    pub async fn local_cancel_delivery(
        &self,
        item: &MailItemId,
    ) -> Result<Option<LocalCancelDelivery>, StorageError> {
        read_cancel(&mut *self.writer.lock().await, item).await
    }
}

pub async fn read_local_cancel_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: &MailItemId,
) -> Result<Option<LocalCancelDelivery>, StorageError> {
    read_cancel(scope.executor(), item).await
}

/// Tentative attribution precedes SDK admission in this same transaction.
pub async fn prepare_local_cancel_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: &MailItemId,
    target: &MailItemId,
    attribution: &CancelCause,
    recorded_at: DateTime<Utc>,
) -> Result<LocalCancelPreparation, StorageError> {
    if read_cancel(scope.executor(), item).await?.is_some() {
        return Ok(LocalCancelPreparation::Kept);
    }
    let attribution_json =
        serde_json::to_string(attribution).map_err(|_| StorageError::InvalidRecord)?;
    let _: CancelCause =
        serde_json::from_str(&attribution_json).map_err(|_| StorageError::InvalidRecord)?;
    let recorded_at = push_record_rows::serialize_timestamp(recorded_at);
    sqlx_turso::query!(
        "INSERT INTO local_cancel_deliveries(item_id,target_item,attribution_json,outcome_json,recorded_at) VALUES (?,?,?,NULL,?)",
        item.as_str(), target.as_str(), attribution_json, recorded_at
    ).execute(scope.executor()).await?;
    Ok(LocalCancelPreparation::Inserted)
}

/// Remove only a row just inserted by this uncommitted admission attempt.
pub async fn delete_tentative_local_cancel_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: &MailItemId,
    preparation: LocalCancelPreparation,
) -> Result<(), StorageError> {
    if preparation == LocalCancelPreparation::Inserted {
        sqlx_turso::query!(
            "DELETE FROM local_cancel_deliveries WHERE item_id=?",
            item.as_str()
        )
        .execute(scope.executor())
        .await?;
    }
    Ok(())
}

/// The SDK control hook updates an existing Local projection, never inventing attribution.
pub async fn record_local_cancel_outcome_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: &MailItemId,
    target: &MailItemId,
    outcome: &RecipientOutcome<CarrierAcceptance>,
    now_ms: i64,
) -> Result<(), StorageError> {
    let Some(record) = read_cancel(scope.executor(), item).await? else {
        return Ok(());
    };
    if record.target_item != *target {
        return Err(StorageError::InvalidStoredRecord);
    }
    validate_outcome(item, outcome)?;
    if record.outcome.as_ref() == Some(outcome) {
        return Ok(());
    }
    let encoded = serde_json::to_string(outcome).map_err(|_| StorageError::InvalidRecord)?;
    let _: RecipientOutcome<CarrierAcceptance> =
        serde_json::from_str(&encoded).map_err(|_| StorageError::InvalidRecord)?;
    sqlx::query::<sqlx_turso::Turso>(
        "UPDATE local_cancel_deliveries SET outcome_json=?,settled_at_ms=? WHERE item_id=?",
    )
    .bind(encoded)
    .bind(now_ms)
    .bind(item.as_str())
    .execute(scope.executor())
    .await?;
    Ok(())
}
