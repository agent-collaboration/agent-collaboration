//! Approval and question pushes keep local carrier semantics in their physical owner.
use crate::{InteractionStore, StorageError, push_record_rows, push_records};
use chrono::{DateTime, Utc};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, PushDeliveryState, PushId, PushKind, PushRecord,
};
use sqlx::{Connection, Transaction};
use sqlx_turso::{Turso, TursoConnection};

impl InteractionStore {
    pub async fn mark_interaction_notice_attempted(
        &self,
        push_id: &PushId,
    ) -> Result<PushRecord, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let changed = async {
            read_interaction_notice_push(&mut transaction, push_id).await?;
            let result = sqlx_turso::query!("UPDATE router_pushes SET delivery_state='attempted',last_outcome_json=NULL,settled_at=NULL WHERE push_id=? AND delivery_state IN ('pending','held')",push_id.as_str()).execute(&mut *transaction).await?;
            if result.rows_affected() != 1 {
                return Err(StorageError::PushStateConflict);
            }
            read_interaction_notice_push(&mut transaction, push_id).await
        }
        .await;
        finish_notice_transaction(transaction, changed).await
    }

    pub async fn settle_interaction_notice(
        &self,
        push_id: &PushId,
        receipt: DeliveryReceipt,
        settled_at: DateTime<Utc>,
    ) -> Result<PushRecord, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let changed = async {
            let current = read_interaction_notice_push(&mut transaction, push_id).await?;
            let (state, label) = match &receipt.outcome {
                DeliveryOutcome::Started
                | DeliveryOutcome::Steered
                | DeliveryOutcome::StartedOrSteered
                | DeliveryOutcome::Queued
                | DeliveryOutcome::PeerMessageWritten => (PushDeliveryState::Delivered, "delivered"),
                DeliveryOutcome::Unknown => (PushDeliveryState::OutcomeUnknown, "outcome_unknown"),
                DeliveryOutcome::NotSubmitted { .. } | DeliveryOutcome::Rejected(_) => {
                    (PushDeliveryState::Rejected, "rejected")
                }
            };
            PushRecord::from_storage_parts(
                push_record_rows::draft_for_record(&current),
                state,
                Some(receipt.clone()),
                Some(settled_at),
                current.read_at,
                current.cancelled_after_delivery.clone(),
            )
            .map_err(|_| StorageError::InvalidRecord)?;
            let encoded = push_record_rows::serialize_json(&receipt)?;
            let settled_at = push_record_rows::serialize_timestamp(settled_at);
            let result = sqlx_turso::query!("UPDATE router_pushes SET delivery_state=?,last_outcome_json=?,settled_at=? WHERE push_id=? AND delivery_state='attempted'",label,encoded,settled_at,push_id.as_str()).execute(&mut *transaction).await?;
            if result.rows_affected() != 1 {
                return Err(StorageError::PushStateConflict);
            }
            read_interaction_notice_push(&mut transaction, push_id).await
        }
        .await;
        finish_notice_transaction(transaction, changed).await
    }

    /// Preserve the previous restart rule: an interrupted native notice is unknown and is never resent.
    pub async fn recover_interrupted_interaction_notices(
        &self,
        now: DateTime<Utc>,
    ) -> Result<u64, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let recovered = async {
            let approval = push_record_rows::serialize_kind(PushKind::Approval)?;
            let question = push_record_rows::serialize_kind(PushKind::Question)?;
            let receipt = DeliveryReceipt {
                outcome: DeliveryOutcome::Unknown,
                reachability: None,
                client: None,
            };
            let encoded = push_record_rows::serialize_json(&receipt)?;
            let settled_at = push_record_rows::serialize_timestamp(now);
            let mut recovered_count = 0;
            loop {
                let notice_push_ids = sqlx_turso::query!("SELECT push_id AS \"push_id!: String\" FROM router_pushes WHERE delivery_state='attempted' AND last_outcome_json IS NULL AND kind IN (?,?) ORDER BY push_id LIMIT 128",approval,question).fetch_all(&mut *transaction).await?;
                if notice_push_ids.is_empty() {
                    break;
                }
                for row in notice_push_ids {
                    let push_id = row
                        .push_id
                        .try_into()
                        .map_err(|_| StorageError::InvalidStoredRecord)?;
                    read_interaction_notice_push(&mut transaction, &push_id).await?;
                    let result = sqlx_turso::query!("UPDATE router_pushes SET delivery_state='outcome_unknown',last_outcome_json=?,settled_at=? WHERE push_id=? AND delivery_state='attempted' AND last_outcome_json IS NULL",encoded,settled_at,push_id.as_str()).execute(&mut *transaction).await?;
                    read_interaction_notice_push(&mut transaction, &push_id).await?;
                    recovered_count += result.rows_affected();
                }
            }
            Ok(recovered_count)
        }
        .await;
        match recovered {
            Ok(count) => {
                transaction.commit().await?;
                Ok(count)
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }
}

async fn read_interaction_notice_push(
    connection: &mut TursoConnection,
    push_id: &PushId,
) -> Result<PushRecord, StorageError> {
    let record = push_records::read_push_with_connection(connection, push_id)
        .await?
        .ok_or(StorageError::PushNotFound)?;
    if !matches!(record.kind, PushKind::Approval | PushKind::Question) {
        return Err(StorageError::PushNotPermitted);
    }
    Ok(record)
}
async fn finish_notice_transaction(
    transaction: Transaction<'_, Turso>,
    result: Result<PushRecord, StorageError>,
) -> Result<PushRecord, StorageError> {
    match result {
        Ok(record) => {
            transaction.commit().await?;
            Ok(record)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

#[cfg(test)]
#[path = "interaction_notice_pushes_tests.rs"]
mod tests;
