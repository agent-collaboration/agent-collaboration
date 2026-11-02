//! The public SDK reservations and the exact pre-I/O listing commit in one owner transaction.
use crate::{StorageError, push_record_rows, push_records};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, PushHeaderFacts, PushId, PushRecord, PushRecordDraft,
};
use durable_steps::{Host, storage::TransactionScope, turso_store::TursoScope};
use std::collections::HashSet;

/// None is the empty reservation: record known-unsent without any native effect.
pub async fn refreeze_notification_listing_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    id: &PushId,
    draft: Option<PushRecordDraft>,
    retained: &[PushId],
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Option<PushRecord>, StorageError> {
    let listing = push_records::read_push_with_connection(scope.executor(), id)
        .await?
        .ok_or(StorageError::PushNotFound)?;
    let original =
        crate::push_records::notification_references::prepare_notification_listing_in_scope(
            scope, id,
        )
        .await?;
    if retained.iter().collect::<HashSet<_>>().len() != retained.len()
        || retained
            .iter()
            .any(|id| !original.iter().any(|member| member.record.push_id == *id))
    {
        return Err(StorageError::InvalidRecord);
    }
    let refrozen = match draft {
        Some(draft) if !retained.is_empty() => {
            let mut expected = push_record_rows::draft_for_record(&listing);
            expected.header_facts = draft.header_facts.clone();
            expected.body = draft.body.clone();
            if draft != expected
                || !matches!(draft.header_facts, PushHeaderFacts::NotificationListing { item_count } if usize::try_from(item_count).ok() == Some(retained.len()))
            {
                return Err(StorageError::InvalidRecord);
            }
            Some(
                draft
                    .into_pending()
                    .map_err(|_| StorageError::InvalidRecord)?,
            )
        }
        None if retained.is_empty() => None,
        _ => return Err(StorageError::InvalidRecord),
    };
    for member in original {
        if !retained.contains(&member.record.push_id) {
            sqlx_turso::query!("UPDATE router_pushes SET notification_listing_push_id=NULL WHERE push_id=? AND notification_listing_push_id=?",member.record.push_id.as_str(),id.as_str()).execute(scope.executor()).await?;
        }
    }
    if let Some(refrozen) = &refrozen {
        let header = push_record_rows::serialize_json(&refrozen.header_facts)?;
        sqlx_turso::query!("UPDATE router_pushes SET header_facts_json=?,body=? WHERE push_id=? AND delivery_state='pending' AND kind='notification-listing'",header,refrozen.body,id.as_str()).execute(scope.executor()).await?;
    }
    let changed=sqlx_turso::query!("UPDATE router_pushes SET delivery_state='attempted',last_outcome_json=NULL,settled_at=NULL WHERE push_id=? AND delivery_state='pending' AND kind='notification-listing'",id.as_str()).execute(scope.executor()).await?;
    if changed.rows_affected() != 1 {
        return Err(StorageError::PushStateConflict);
    }
    if refrozen.is_none() {
        crate::push_records::notification_settlement::settle_listing(
            scope.executor(),
            id,
            DeliveryReceipt {
                outcome: DeliveryOutcome::NotSubmitted {
                    retryable: true,
                    reason: "no notification member reserved for native delivery".to_owned(),
                },
                reachability: None,
                client: None,
            },
            now,
        )
        .await?;
        return Ok(None);
    }
    push_records::read_push_with_connection(scope.executor(), id).await
}
