//! Local attribution accompanies SDK custody; it never decides replay or conflict.
use crate::{InteractionStore, StorageError, push_record_rows};
use chrono::{DateTime, Utc};
use collaboration_mail_protocol::{CancelCause, CarrierAcceptance, MailItemId};
use durable_steps::{Host, storage::TransactionScope, turso_store::TursoScope};
use fleet_sync_protocol::RecipientOutcome;
use sqlx_turso::TursoConnection;

#[derive(Clone, Debug)]
pub struct LocalCancelDelivery {
    pub item_id: MailItemId,
    pub target_item: MailItemId,
    pub attribution: CancelCause,
    pub outcome: Option<RecipientOutcome<CarrierAcceptance>>,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalCancelPreparation {
    Inserted,
    Kept,
}

struct LocalCancelRow {
    item_id: String,
    target_item: String,
    attribution_json: String,
    outcome_json: Option<String>,
    recorded_at: String,
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
        Ok(LocalCancelDelivery {
            item_id,
            target_item,
            attribution,
            outcome,
            recorded_at: push_record_rows::decode_timestamp(self.recorded_at)
                .map_err(|_| StorageError::InvalidStoredRecord)?,
        })
    }
}

async fn read_cancel(
    connection: &mut TursoConnection,
    item: &MailItemId,
) -> Result<Option<LocalCancelDelivery>, StorageError> {
    sqlx_turso::query_as!(LocalCancelRow,
        "SELECT item_id AS \"item_id!: String\",target_item AS \"target_item!: String\",attribution_json AS \"attribution_json!: String\",outcome_json,recorded_at AS \"recorded_at!: String\" FROM local_cancel_deliveries WHERE item_id=?",
        item.as_str())
        .fetch_optional(connection).await?
        .map(LocalCancelRow::decode).transpose()
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
) -> Result<(), StorageError> {
    let Some(record) = read_cancel(scope.executor(), item).await? else {
        return Ok(());
    };
    if record.target_item != *target {
        return Err(StorageError::InvalidStoredRecord);
    }
    validate_outcome(item, outcome)?;
    let encoded = serde_json::to_string(outcome).map_err(|_| StorageError::InvalidRecord)?;
    let _: RecipientOutcome<CarrierAcceptance> =
        serde_json::from_str(&encoded).map_err(|_| StorageError::InvalidRecord)?;
    sqlx_turso::query!(
        "UPDATE local_cancel_deliveries SET outcome_json=? WHERE item_id=?",
        encoded,
        item.as_str()
    )
    .execute(scope.executor())
    .await?;
    Ok(())
}
