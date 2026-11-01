//! One receipt settles the Router listing projection; SDK items replay it individually.
use crate::push_records::notification_listings::finish_listing_transaction;
use crate::push_records::notification_windows::write_notification_delivery;
use crate::{InteractionStore, StorageError, push_record_rows, push_records};
use chrono::{DateTime, Utc};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, PushDeliveryState, PushId, PushKind, PushRecord, SessionRef,
};
use sqlx::Connection;
use sqlx_turso::TursoConnection;

struct ListingMemberIdRow {
    push_id: String,
}
struct NotificationTargetRow {
    service: String,
    endpoint: String,
    session: String,
}

impl InteractionStore {
    pub async fn settle_notification_listing(
        &self,
        id: &PushId,
        receipt: DeliveryReceipt,
        now: DateTime<Utc>,
    ) -> Result<Vec<PushId>, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let result = settle_listing(&mut transaction, id, receipt, now).await;
        finish_listing_transaction(transaction, result).await
    }
    pub async fn recover_interrupted_notification_listings(
        &self,
        target: &SessionRef,
        startup_attempts: &std::collections::HashSet<PushId>,
        now: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            let mut window = crate::push_records::notification_windows::read_notification_window(&mut transaction, target).await?;
            let service=String::from(target.endpoint.service_id.clone());
            let endpoint=String::from(target.endpoint.endpoint_id.clone());
            let session=String::from(target.session_id.clone());
            let ids=sqlx_turso::query_as!(ListingMemberIdRow,"SELECT push_id AS \"push_id!: String\" FROM router_pushes WHERE target_service_id=? AND target_endpoint_id=? AND target_session_id=? AND kind IN ('notification-listing','board-notification','subscription-activity','subscription-expiry') AND delivery_state IN ('attempted','outcome_unknown') ORDER BY push_id",service,endpoint,session).fetch_all(&mut *transaction).await?;
            for row in ids {
                let id=PushId::try_from(row.push_id).map_err(|_|StorageError::InvalidStoredRecord)?;
                let record=push_records::read_push_with_connection(&mut transaction,&id).await?.ok_or(StorageError::InvalidStoredRecord)?;
                if record.delivery_state==PushDeliveryState::Attempted && record.kind!=PushKind::NotificationListing && !startup_attempts.contains(&id) {
                    continue;
                }
                let recovered_at = if record.delivery_state==PushDeliveryState::Attempted { now } else { record.settled_at.ok_or(StorageError::InvalidStoredRecord)? };
                // SDK or carrier recovery may already have recorded Unknown. Its
                // original settlement is the idempotent lower bound for the window.
                if window.as_ref().is_none_or(|window| window.last_delivered_at < recovered_at) {
                    write_notification_delivery(&mut transaction,target,recovered_at).await?;
                    window=crate::push_records::notification_windows::read_notification_window(&mut transaction,target).await?;
                }
                if record.kind==PushKind::NotificationListing && record.delivery_state==PushDeliveryState::Attempted {
                    settle_listing(&mut transaction,&id,DeliveryReceipt{outcome:DeliveryOutcome::Unknown,reachability:None,client:None},now).await?;
                } else if record.delivery_state==PushDeliveryState::Attempted {
                    let outcome_json=push_record_rows::serialize_json(&DeliveryReceipt{outcome:DeliveryOutcome::Unknown,reachability:None,client:None})?;
                    let settled_at=push_record_rows::serialize_timestamp(now);
                    sqlx_turso::query!("UPDATE router_pushes SET delivery_state='outcome_unknown',last_outcome_json=?,settled_at=? WHERE retention='creationAge' AND push_id=? AND delivery_state='attempted' AND last_outcome_json IS NULL",outcome_json,settled_at,id.as_str()).execute(&mut *transaction).await?;
                }
            }
            Ok(())
        }.await;
        finish_listing_transaction(transaction, result).await
    }
    pub async fn notification_recovery_targets(&self) -> Result<Vec<SessionRef>, StorageError> {
        let mut connection = self.writer.lock().await;
        let targets=sqlx_turso::query_as!(NotificationTargetRow,"SELECT DISTINCT target_service_id AS \"service!: String\",target_endpoint_id AS \"endpoint!: String\",target_session_id AS \"session!: String\" FROM router_pushes WHERE kind IN ('notification-listing','board-notification','subscription-activity','subscription-expiry') AND delivery_state IN ('pending','held','attempted') ORDER BY target_service_id,target_endpoint_id,target_session_id").fetch_all(&mut *connection).await?;
        let mut recipients = Vec::with_capacity(targets.len());
        for row in targets {
            match push_record_rows::session_from_parts(row.service, row.endpoint, row.session) {
                Ok(recipient) => recipients.push(recipient),
                Err(error) => {
                    tracing::warn!(%error, "invalid notification recovery recipient rejected")
                }
            }
        }
        Ok(recipients)
    }
}
pub(crate) async fn settle_listing(
    connection: &mut TursoConnection,
    id: &PushId,
    receipt: DeliveryReceipt,
    now: DateTime<Utc>,
) -> Result<Vec<PushId>, StorageError> {
    let listing = push_records::read_push_with_connection(connection, id)
        .await?
        .ok_or(StorageError::PushNotFound)?;
    if listing.kind != PushKind::NotificationListing {
        return Err(StorageError::InvalidRecord);
    }
    let state = match &receipt.outcome {
        DeliveryOutcome::Started
        | DeliveryOutcome::Steered
        | DeliveryOutcome::StartedOrSteered
        | DeliveryOutcome::Queued
        | DeliveryOutcome::PeerMessageWritten => PushDeliveryState::Delivered,
        DeliveryOutcome::Unknown => PushDeliveryState::OutcomeUnknown,
        DeliveryOutcome::Rejected(_) | DeliveryOutcome::NotSubmitted { .. } => {
            PushDeliveryState::Rejected
        }
    };
    let rows=sqlx_turso::query_as!(ListingMemberIdRow,"SELECT push_id AS \"push_id!: String\" FROM router_pushes WHERE notification_listing_push_id=? ORDER BY push_id",id.as_str()).fetch_all(&mut *connection).await?;
    let mut ids = Vec::with_capacity(rows.len());
    for row in rows {
        ids.push(PushId::try_from(row.push_id).map_err(|_| StorageError::InvalidStoredRecord)?);
    }
    let encoded = push_record_rows::serialize_json(&receipt)?;
    if listing.delivery_state != PushDeliveryState::Attempted {
        return if listing.delivery_state == state
            && listing
                .last_outcome
                .as_ref()
                .map(push_record_rows::serialize_json)
                .transpose()?
                .as_deref()
                == Some(encoded.as_str())
        {
            Ok(ids)
        } else {
            Err(StorageError::PushStateConflict)
        };
    }
    let timestamp = push_record_rows::serialize_timestamp(now);
    let label = push_record_rows::serialize_delivery_state(state);
    for member_id in &ids {
        let member = push_records::read_push_with_connection(connection, member_id)
            .await?
            .ok_or(StorageError::InvalidStoredRecord)?;
        if member.target != listing.target
            || !crate::push_records::notification_listings::is_notification_kind(member.kind)
        {
            return Err(StorageError::InvalidStoredRecord);
        }
        if state == PushDeliveryState::Rejected {
            if crate::push_records::notification_references::notification_reference(
                connection, member_id,
            )
            .await?
            .is_none()
            {
                sqlx_turso::query!("UPDATE router_pushes SET notification_listing_push_id=NULL WHERE push_id=? AND notification_listing_push_id=?",member_id.as_str(),id.as_str()).execute(&mut *connection).await?;
            }
        } else {
            validate_terminal_member(&member, state, &receipt, now)?;
            sqlx_turso::query!("UPDATE router_pushes SET delivery_state=?,last_outcome_json=?,settled_at=?,last_outcome_at=? WHERE push_id=? AND notification_listing_push_id=?",label,encoded,timestamp,timestamp,member_id.as_str(),id.as_str()).execute(&mut *connection).await?;
        }
    }
    sqlx_turso::query!("UPDATE router_pushes SET delivery_state=?,last_outcome_json=?,settled_at=?,last_outcome_at=? WHERE push_id=? AND kind='notification-listing' AND delivery_state='attempted'",label,encoded,timestamp,timestamp,id.as_str()).execute(&mut *connection).await?;
    if state != PushDeliveryState::Rejected {
        write_notification_delivery(connection, &listing.target, now).await?;
    }
    Ok(ids)
}
fn validate_terminal_member(
    member: &PushRecord,
    state: PushDeliveryState,
    receipt: &DeliveryReceipt,
    now: DateTime<Utc>,
) -> Result<(), StorageError> {
    PushRecord::from_storage_parts(
        push_record_rows::draft_for_record(member),
        state,
        Some(receipt.clone()),
        Some(now),
        member.read_at,
        member.cancelled_after_delivery.clone(),
    )
    .map_err(|_| StorageError::InvalidRecord)?;
    Ok(())
}
