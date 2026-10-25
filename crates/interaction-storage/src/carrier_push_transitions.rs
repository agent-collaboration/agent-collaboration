//! Creation-age carrier transitions; SDK mail is writable only through its scoped projection.
use crate::push_record_rows;
use crate::{InteractionStore, StorageError};
use chrono::{DateTime, Utc};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, MessageDelivery, PushDeliveryState, PushId, PushKind,
    PushRecord,
};
use sqlx::{Connection, Transaction};
use sqlx_turso::Turso;
impl InteractionStore {
    pub async fn mark_push_attempted(&self, push_id: &PushId) -> Result<PushRecord, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let _ = crate::push_records::read_push_with_connection(&mut transaction, push_id).await?;
        let result = sqlx_turso::query!(
            "UPDATE router_pushes SET delivery_state='attempted',last_outcome_json=NULL,settled_at=NULL WHERE retention='creationAge' AND push_id=? AND delivery_state IN ('pending','held')",
            push_id.as_str()
        )
        .execute(&mut *transaction)
        .await?;
        finish_transition(transaction, push_id, result.rows_affected()).await
    }

    pub async fn restore_push_pending_after_not_started(
        &self,
        push_id: &PushId,
    ) -> Result<PushRecord, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let _ = crate::push_records::read_push_with_connection(&mut transaction, push_id).await?;
        let result = sqlx_turso::query!(
            "UPDATE router_pushes SET delivery_state='pending',last_outcome_json=NULL,settled_at=NULL WHERE retention='creationAge' AND push_id=? AND delivery_state='attempted'",
            push_id.as_str()
        )
        .execute(&mut *transaction)
        .await?;
        finish_transition(transaction, push_id, result.rows_affected()).await
    }

    pub async fn hold_push_record(
        &self,
        push_id: &PushId,
        outcome: DeliveryReceipt,
    ) -> Result<PushRecord, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        if !matches!(
            outcome.outcome,
            DeliveryOutcome::NotSubmitted {
                retryable: true,
                ..
            }
        ) {
            return Err(StorageError::InvalidRecord);
        }
        let record = crate::push_records::read_push_with_connection(&mut transaction, push_id)
            .await?
            .ok_or(StorageError::PushNotFound)?;
        if record.kind == PushKind::DirectMessage
            && (record.mode == Some(collaboration_protocol::MessageDelivery::Steer)
                || record.guard.is_some())
        {
            return Err(StorageError::InvalidRecord);
        }
        let outcome_json = push_record_rows::serialize_json(&outcome)?;
        let result = sqlx_turso::query!(
            "UPDATE router_pushes SET delivery_state='held',last_outcome_json=?,settled_at=NULL WHERE retention='creationAge' AND push_id=? AND delivery_state='attempted'",
            outcome_json,
            push_id.as_str()
        )
        .execute(&mut *transaction)
        .await?;
        finish_transition(transaction, push_id, result.rows_affected()).await
    }

    pub async fn settle_push_record(
        &self,
        push_id: &PushId,
        outcome: DeliveryReceipt,
        settled_at: DateTime<Utc>,
    ) -> Result<PushRecord, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let _ = crate::push_records::read_push_with_connection(&mut transaction, push_id).await?;
        let state = settled_state(&outcome.outcome);
        let outcome_json = push_record_rows::serialize_json(&outcome)?;
        let settled_at = push_record_rows::serialize_timestamp(settled_at);
        let result = sqlx_turso::query!(
            "UPDATE router_pushes SET delivery_state=?,last_outcome_json=?,settled_at=? WHERE retention='creationAge' AND push_id=? AND delivery_state='attempted'",
            push_record_rows::serialize_delivery_state(state),
            outcome_json,
            settled_at,
            push_id.as_str()
        )
        .execute(&mut *transaction)
        .await?;
        finish_transition(transaction, push_id, result.rows_affected()).await
    }

    /// A presence check made no submission. Keep that distinction durable across a crash.
    pub async fn hold_unsent_direct_message(
        &self,
        push_id: &PushId,
        receipt: DeliveryReceipt,
    ) -> Result<PushRecord, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let record = crate::push_records::read_push_with_connection(&mut transaction, push_id)
            .await?
            .ok_or(StorageError::PushNotFound)?;
        let holdable = match record.kind {
            PushKind::DirectMessage => matches!(
                record.mode,
                Some(MessageDelivery::Auto | MessageDelivery::Queue)
            ),
            PushKind::BoardNotification => record.mode.is_none(),
            _ => false,
        };
        if !holdable
            || record.guard.is_some()
            || !matches!(
                receipt.outcome,
                DeliveryOutcome::NotSubmitted {
                    retryable: true,
                    ..
                }
            )
        {
            return Err(StorageError::InvalidRecord);
        }
        let kind = push_record_rows::serialize_kind(PushKind::DirectMessage)?;
        let board = push_record_rows::serialize_kind(PushKind::BoardNotification)?;
        let outcome_json = push_record_rows::serialize_json(&receipt)?;
        let result = sqlx_turso::query!(
            "UPDATE router_pushes SET delivery_state='held',last_outcome_json=?,settled_at=NULL WHERE retention='creationAge' AND push_id=? AND kind IN (?,?) AND delivery_state IN ('pending','held')",
            outcome_json, push_id.as_str(), kind, board
        ).execute(&mut *transaction).await?;
        if result.rows_affected() != 1 {
            return Err(StorageError::PushStateConflict);
        }
        finish_transition(transaction, push_id, result.rows_affected()).await
    }

    /// Reject known-unsent work without inventing an attempted effect.
    pub async fn reject_unsent_direct_message(
        &self,
        push_id: &PushId,
        receipt: DeliveryReceipt,
        now: DateTime<Utc>,
    ) -> Result<PushRecord, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let _ = crate::push_records::read_push_with_connection(&mut transaction, push_id).await?;
        if !matches!(
            receipt.outcome,
            DeliveryOutcome::Rejected(_) | DeliveryOutcome::NotSubmitted { .. }
        ) {
            return Err(StorageError::InvalidRecord);
        }
        let kind = push_record_rows::serialize_kind(PushKind::DirectMessage)?;
        let board = push_record_rows::serialize_kind(PushKind::BoardNotification)?;
        let outcome_json = push_record_rows::serialize_json(&receipt)?;
        let settled_at = push_record_rows::serialize_timestamp(now);
        let result = sqlx_turso::query!(
            "UPDATE router_pushes SET delivery_state='rejected',last_outcome_json=?,settled_at=? WHERE retention='creationAge' AND push_id=? AND kind IN (?,?) AND delivery_state IN ('pending','held')",
            outcome_json, settled_at, push_id.as_str(), kind, board
        ).execute(&mut *transaction).await?;
        if result.rows_affected() != 1 {
            return Err(StorageError::PushStateConflict);
        }
        finish_transition(transaction, push_id, result.rows_affected()).await
    }
}

async fn finish_transition(
    mut transaction: Transaction<'_, Turso>,
    id: &PushId,
    changed: u64,
) -> Result<PushRecord, StorageError> {
    let record = crate::push_records::read_push_with_connection(&mut transaction, id).await?;
    if changed != 1 {
        return Err(if record.is_none() {
            StorageError::PushNotFound
        } else {
            StorageError::PushStateConflict
        });
    }
    let record = record.ok_or(StorageError::PushNotFound)?;
    transaction.commit().await?;
    Ok(record)
}
fn settled_state(outcome: &DeliveryOutcome) -> PushDeliveryState {
    match outcome {
        DeliveryOutcome::Started
        | DeliveryOutcome::Steered
        | DeliveryOutcome::StartedOrSteered
        | DeliveryOutcome::Queued
        | DeliveryOutcome::PeerMessageWritten => PushDeliveryState::Delivered,
        DeliveryOutcome::Unknown => PushDeliveryState::OutcomeUnknown,
        DeliveryOutcome::NotSubmitted { .. } | DeliveryOutcome::Rejected(_) => {
            PushDeliveryState::Rejected
        }
    }
}
