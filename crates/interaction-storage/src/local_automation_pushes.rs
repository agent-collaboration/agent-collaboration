//! Local wake and scheduled-run effects use the physical owner without cross-machine custody.
use crate::{InteractionStore, PushRecord, StorageError, push_record_rows, push_records};
use chrono::{DateTime, Utc};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, PushDeliveryState, PushId, PushKind, RouterOriginRef,
};
use sqlx::{Connection, Transaction};
use sqlx_turso::{Turso, TursoConnection};

impl InteractionStore {
    pub async fn mark_wake_push_attempted(
        &self,
        push_id: &PushId,
    ) -> Result<PushRecord, StorageError> {
        self.mark_automation_push_attempted(push_id, PushKind::Wake)
            .await
    }
    pub async fn mark_schedule_push_attempted(
        &self,
        push_id: &PushId,
    ) -> Result<PushRecord, StorageError> {
        self.mark_automation_push_attempted(push_id, PushKind::ScheduleRun)
            .await
    }
    async fn mark_automation_push_attempted(
        &self,
        push_id: &PushId,
        kind: PushKind,
    ) -> Result<PushRecord, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let changed = async {
            read_local_automation_push(&mut transaction,push_id,kind).await?;
            let result = sqlx_turso::query!("UPDATE router_pushes SET delivery_state='attempted',last_outcome_json=NULL,settled_at=NULL WHERE push_id=? AND delivery_state IN ('pending','held')",push_id.as_str()).execute(&mut *transaction).await?;
            if result.rows_affected() != 1 {
                return Err(StorageError::PushStateConflict);
            }
            read_local_automation_push(&mut transaction,push_id,kind).await
        }
        .await;
        finish_push_transaction(transaction, changed).await
    }

    pub async fn restore_wake_push_pending(
        &self,
        push_id: &PushId,
    ) -> Result<PushRecord, StorageError> {
        self.restore_automation_push_pending(push_id, PushKind::Wake)
            .await
    }

    pub async fn restore_schedule_push_pending(
        &self,
        push_id: &PushId,
    ) -> Result<PushRecord, StorageError> {
        self.restore_automation_push_pending(push_id, PushKind::ScheduleRun)
            .await
    }

    async fn restore_automation_push_pending(
        &self,
        push_id: &PushId,
        kind: PushKind,
    ) -> Result<PushRecord, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let changed = async {
            read_local_automation_push(&mut transaction,push_id,kind).await?;
            let result = sqlx_turso::query!("UPDATE router_pushes SET delivery_state='pending',last_outcome_json=NULL,settled_at=NULL WHERE push_id=? AND delivery_state='attempted'",push_id.as_str()).execute(&mut *transaction).await?;
            if result.rows_affected() != 1 {
                return Err(StorageError::PushStateConflict);
            }
            read_local_automation_push(&mut transaction,push_id,kind).await
        }
        .await;
        finish_push_transaction(transaction, changed).await
    }

    pub async fn settle_wake_push(
        &self,
        push_id: &PushId,
        receipt: DeliveryReceipt,
        settled_at: DateTime<Utc>,
    ) -> Result<PushRecord, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let changed = async {
            let current=read_local_automation_push(&mut transaction,push_id,PushKind::Wake).await?;
            let state=match &receipt.outcome {
                DeliveryOutcome::Started|DeliveryOutcome::Steered|DeliveryOutcome::StartedOrSteered|DeliveryOutcome::Queued|DeliveryOutcome::PeerMessageWritten=>PushDeliveryState::Delivered,
                DeliveryOutcome::Unknown=>PushDeliveryState::OutcomeUnknown,
                DeliveryOutcome::NotSubmitted {..}|DeliveryOutcome::Rejected(_)=>PushDeliveryState::Rejected,
            };
            validate_push_outcome(&current,state,&receipt,settled_at)?;
            let state=push_state_label(state)?;
            let encoded=push_record_rows::serialize_json(&receipt)?;
            let settled_at=push_record_rows::serialize_timestamp(settled_at);
            let result = sqlx_turso::query!("UPDATE router_pushes SET delivery_state=?,last_outcome_json=?,settled_at=? WHERE push_id=? AND delivery_state='attempted'",state,encoded,settled_at,push_id.as_str()).execute(&mut *transaction).await?;
            if result.rows_affected() != 1 {
                return Err(StorageError::PushStateConflict);
            }
            read_local_automation_push(&mut transaction,push_id,PushKind::Wake).await
        }
        .await;
        finish_push_transaction(transaction, changed).await
    }

    /// Applies one automation-owed settlement, preserving an identical recorded result on replay.
    pub async fn apply_schedule_run_push_outcome(
        &self,
        push_id: &PushId,
        origin: &RouterOriginRef,
        state: PushDeliveryState,
        receipt: &DeliveryReceipt,
        settled_at: DateTime<Utc>,
    ) -> Result<PushRecord, StorageError> {
        if !matches!(origin, RouterOriginRef::ScheduleRun { .. })
            || !matches!(
                state,
                PushDeliveryState::Pending
                    | PushDeliveryState::Delivered
                    | PushDeliveryState::OutcomeUnknown
            )
        {
            return Err(StorageError::InvalidRecord);
        }
        let expected_origin = origin
            .canonical_string()
            .map_err(|_| StorageError::InvalidRecord)?;
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let changed = async {
            let current=read_local_automation_push(&mut transaction,push_id,PushKind::ScheduleRun).await?;
            if current.origin_router_ref.as_deref() != Some(expected_origin.as_str()) {
                return Err(StorageError::PushContentConflict);
            }
            validate_push_outcome(&current,state,receipt,settled_at)?;
            let encoded=push_record_rows::serialize_json(receipt)?;
            if current.delivery_state==state && current.settled_at==Some(settled_at)
                && current.last_outcome.as_ref().map(push_record_rows::serialize_json).transpose()?.as_deref()==Some(encoded.as_str()) {
                return Ok(current);
            }
            if current.delivery_state != PushDeliveryState::Attempted {
                return Err(StorageError::PushStateConflict);
            }
            let state=push_state_label(state)?;
            let settled_at=push_record_rows::serialize_timestamp(settled_at);
            let result = sqlx_turso::query!("UPDATE router_pushes SET delivery_state=?,last_outcome_json=?,settled_at=? WHERE push_id=? AND origin_kind='router' AND origin_router_ref=? AND delivery_state='attempted'",state,encoded,settled_at,push_id.as_str(),expected_origin).execute(&mut *transaction).await?;
            if result.rows_affected() != 1 {
                return Err(StorageError::PushStateConflict);
            }
            read_local_automation_push(&mut transaction,push_id,PushKind::ScheduleRun).await
        }
        .await;
        finish_push_transaction(transaction, changed).await
    }
}

async fn read_local_automation_push(
    connection: &mut TursoConnection,
    push_id: &PushId,
    kind: PushKind,
) -> Result<PushRecord, StorageError> {
    let record = push_records::read_push_with_connection(connection, push_id)
        .await?
        .ok_or(StorageError::PushNotFound)?;
    if record.kind != kind {
        return Err(StorageError::PushNotPermitted);
    }
    Ok(record)
}
fn validate_push_outcome(
    record: &PushRecord,
    state: PushDeliveryState,
    receipt: &DeliveryReceipt,
    settled_at: DateTime<Utc>,
) -> Result<(), StorageError> {
    PushRecord::from_storage_parts(
        push_record_rows::draft_for_record(record),
        state,
        Some(receipt.clone()),
        Some(settled_at),
        record.read_at,
        record.cancelled_after_delivery.clone(),
    )
    .map_err(|_| StorageError::InvalidRecord)?;
    Ok(())
}
fn push_state_label(state: PushDeliveryState) -> Result<&'static str, StorageError> {
    match state {
        PushDeliveryState::Pending => Ok("pending"),
        PushDeliveryState::Delivered => Ok("delivered"),
        PushDeliveryState::OutcomeUnknown => Ok("outcome_unknown"),
        PushDeliveryState::Rejected => Ok("rejected"),
        PushDeliveryState::Attempted | PushDeliveryState::Held => Err(StorageError::InvalidRecord),
    }
}
async fn finish_push_transaction(
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
