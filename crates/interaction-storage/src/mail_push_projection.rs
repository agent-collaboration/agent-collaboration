//! Mail pushes change only inside the library's borrowed admission or outcome transaction.
use crate::{StorageError, push_record_rows, push_records, push_retention::PushRetention};
use chrono::{DateTime, Utc};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, PushDeliveryState, PushHeaderFacts, PushId, PushKind,
    PushRecord, PushRecordDraft,
};
use durable_steps::{Host, storage::TransactionScope, turso_store::TursoScope};

pub enum MailPushUpdate {
    Held {
        receipt: DeliveryReceipt,
        held_at: DateTime<Utc>,
    },
    Finished {
        receipt: DeliveryReceipt,
        settled_at: DateTime<Utc>,
    },
}

pub async fn insert_mail_push_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    draft: PushRecordDraft,
) -> Result<(), StorageError> {
    let record = draft
        .into_pending()
        .map_err(|_| StorageError::InvalidRecord)?;
    validate_mail_kind(record.kind)?;
    push_records::insert_push_record_with_retention(
        scope.executor(),
        &record,
        PushRetention::MailTerminalAge,
    )
    .await
}

pub async fn read_mail_push_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    push_id: &PushId,
) -> Result<Option<PushRecord>, StorageError> {
    let record = push_records::read_push_with_connection(scope.executor(), push_id).await?;
    if let Some(record) = &record {
        validate_mail_kind(record.kind)?;
    }
    Ok(record)
}

/// Claim only a still-eligible readiness hold in the transaction releasing SDK custody.
pub async fn mark_released_mail_pending_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    push_id: &PushId,
) -> Result<bool, StorageError> {
    let changed = sqlx_turso::query!(
        "UPDATE router_pushes SET delivery_state='pending',last_outcome_json=NULL,settled_at=NULL WHERE push_id=? AND retention='mailTerminalAge' AND delivery_state='held' AND NOT EXISTS (SELECT 1 FROM local_delivery_retries r WHERE r.item_id=router_pushes.push_id AND r.scheduled_retry_at IS NOT NULL)",
        push_id.as_str()
    ).execute(scope.executor()).await?;
    Ok(changed.rows_affected() == 1)
}

pub async fn mark_mail_push_attempted_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    push_id: &PushId,
) -> Result<PushRecord, StorageError> {
    let current = read_mail_push_in_scope(scope, push_id)
        .await?
        .ok_or(StorageError::PushNotFound)?;
    if !matches!(
        current.delivery_state,
        PushDeliveryState::Pending | PushDeliveryState::Held
    ) {
        return Err(StorageError::PushStateConflict);
    }
    let changed=sqlx_turso::query!("UPDATE router_pushes SET delivery_state='attempted',last_outcome_json=NULL,settled_at=NULL WHERE push_id=? AND delivery_state IN ('pending','held')",push_id.as_str()).execute(scope.executor()).await?;
    if changed.rows_affected() != 1 {
        return Err(StorageError::PushStateConflict);
    }
    read_mail_push_in_scope(scope, push_id)
        .await?
        .ok_or(StorageError::PushNotFound)
}

/// A pruned push stays absent. Replaying the same terminal receipt preserves its first settlement.
pub async fn apply_mail_push_outcome_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    push_id: &PushId,
    update: MailPushUpdate,
) -> Result<Option<PushRecord>, StorageError> {
    let Some(current) = read_mail_push_in_scope(scope, push_id).await? else {
        return Ok(None);
    };
    let mut draft = push_record_rows::draft_for_record(&current);
    let (state, receipt, settled_at, recorded_at) = match update {
        MailPushUpdate::Held { receipt, held_at } => {
            if let (PushHeaderFacts::SubscriptionActivity { held_since, .. }, Some(activity)) =
                (&mut draft.header_facts, &mut draft.activity)
            {
                held_since.get_or_insert_with(|| held_at.to_rfc3339());
                activity.held = true;
            }
            (PushDeliveryState::Held, receipt, None, held_at)
        }
        MailPushUpdate::Finished {
            receipt,
            settled_at,
        } => {
            let state = match &receipt.outcome {
                DeliveryOutcome::Started
                | DeliveryOutcome::Steered
                | DeliveryOutcome::StartedOrSteered
                | DeliveryOutcome::Queued
                | DeliveryOutcome::PeerMessageWritten => PushDeliveryState::Delivered,
                DeliveryOutcome::Unknown => PushDeliveryState::OutcomeUnknown,
                DeliveryOutcome::NotSubmitted { .. } | DeliveryOutcome::Rejected(_) => {
                    PushDeliveryState::Rejected
                }
            };
            (state, receipt, Some(settled_at), settled_at)
        }
    };
    let encoded = push_record_rows::serialize_json(&receipt)?;
    if matches!(
        current.delivery_state,
        PushDeliveryState::Delivered
            | PushDeliveryState::OutcomeUnknown
            | PushDeliveryState::Rejected
    ) {
        if current.delivery_state == state
            && current
                .last_outcome
                .as_ref()
                .map(push_record_rows::serialize_json)
                .transpose()?
                .as_deref()
                == Some(encoded.as_str())
        {
            return Ok(Some(current));
        }
        return Err(StorageError::PushStateConflict);
    }
    if state == PushDeliveryState::Delivered
        && current.delivery_state != PushDeliveryState::Attempted
    {
        return Err(StorageError::PushStateConflict);
    }
    let updated = PushRecord::from_storage_parts(
        draft,
        state,
        Some(receipt),
        settled_at,
        current.read_at,
        current.cancelled_after_delivery.clone(),
    )
    .map_err(|_| StorageError::InvalidRecord)?;
    let header = push_record_rows::serialize_json(&updated.header_facts)?;
    let activity = updated
        .activity
        .as_ref()
        .map(push_record_rows::serialize_json)
        .transpose()?;
    let settled = updated
        .settled_at
        .map(push_record_rows::serialize_timestamp);
    let state = state_label(updated.delivery_state);
    let previous_state = state_label(current.delivery_state);
    let observed_at = push_record_rows::serialize_timestamp(recorded_at);
    let changed=sqlx_turso::query!("UPDATE router_pushes SET delivery_state=?,last_outcome_json=?,settled_at=?,header_facts_json=?,ranges_json=?,last_outcome_at=? WHERE push_id=? AND delivery_state=?",state,encoded,settled,header,activity,observed_at,push_id.as_str(),previous_state).execute(scope.executor()).await?;
    if changed.rows_affected() != 1 {
        return Err(StorageError::PushStateConflict);
    }
    Ok(Some(updated))
}

fn validate_mail_kind(kind: PushKind) -> Result<(), StorageError> {
    match kind {
        PushKind::DirectMessage | PushKind::SubscriptionActivity | PushKind::BoardNotification => {
            Ok(())
        }
        _ => Err(StorageError::PushNotPermitted),
    }
}
fn state_label(state: PushDeliveryState) -> &'static str {
    match state {
        PushDeliveryState::Pending => "pending",
        PushDeliveryState::Attempted => "attempted",
        PushDeliveryState::Held => "held",
        PushDeliveryState::Delivered => "delivered",
        PushDeliveryState::OutcomeUnknown => "outcome_unknown",
        PushDeliveryState::Rejected => "rejected",
    }
}
