//! Frozen listing membership stays on original pushes; SDK custody remains untouched.
use crate::{InteractionStore, StorageError, push_records};
use collaboration_protocol::{
    PushDeliveryState, PushId, PushKind, PushRecord, PushRecordDraft, SessionRef,
};
use sqlx::{Connection, Transaction};
use sqlx_turso::{Turso, TursoConnection};
use std::collections::HashSet;

struct NotificationPushIdRow {
    push_id: String,
}
struct NotificationListingRefRow {
    notification_listing_push_id: Option<String>,
}

pub fn is_notification_kind(kind: PushKind) -> bool {
    matches!(
        kind,
        PushKind::BoardNotification | PushKind::SubscriptionActivity | PushKind::SubscriptionExpiry
    )
}
async fn decode_ids(
    rows: Vec<NotificationPushIdRow>,
    connection: &mut TursoConnection,
) -> Result<Vec<PushRecord>, StorageError> {
    let mut records = Vec::with_capacity(rows.len());
    for row in rows {
        let id = PushId::try_from(row.push_id).map_err(|_| StorageError::InvalidStoredRecord)?;
        records.push(
            push_records::read_push_with_connection(connection, &id)
                .await?
                .ok_or(StorageError::PushNotFound)?,
        );
    }
    Ok(records)
}
pub(crate) async fn listing_for_member(
    connection: &mut TursoConnection,
    id: &PushId,
) -> Result<Option<PushRecord>, StorageError> {
    let row = sqlx_turso::query_as!(
        NotificationListingRefRow,
        "SELECT notification_listing_push_id FROM router_pushes WHERE push_id=?",
        id.as_str()
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some(listing) = row.and_then(|row| row.notification_listing_push_id) else {
        return Ok(None);
    };
    let listing = PushId::try_from(listing).map_err(|_| StorageError::InvalidStoredRecord)?;
    let listing = push_records::read_push_with_connection(connection, &listing)
        .await?
        .ok_or(StorageError::InvalidStoredRecord)?;
    let member = push_records::read_push_with_connection(connection, id)
        .await?
        .ok_or(StorageError::PushNotFound)?;
    if listing.kind != PushKind::NotificationListing
        || listing.target != member.target
        || !is_notification_kind(member.kind)
    {
        return Err(StorageError::InvalidStoredRecord);
    }
    Ok(Some(listing))
}
impl InteractionStore {
    pub async fn notification_listing_for_member(
        &self,
        id: &PushId,
    ) -> Result<Option<PushRecord>, StorageError> {
        listing_for_member(&mut *self.writer.lock().await, id).await
    }
    pub async fn held_notification_candidates(
        &self,
        target: &SessionRef,
    ) -> Result<Vec<PushRecord>, StorageError> {
        let mut connection = self.writer.lock().await;
        let service = String::from(target.endpoint.service_id.clone());
        let endpoint = String::from(target.endpoint.endpoint_id.clone());
        let session = String::from(target.session_id.clone());
        let rows=sqlx_turso::query_as!(NotificationPushIdRow,"SELECT push_id AS \"push_id!: String\" FROM router_pushes WHERE target_service_id=? AND target_endpoint_id=? AND target_session_id=? AND kind IN ('board-notification','subscription-activity','subscription-expiry') AND delivery_state='held' AND (retention='creationAge' OR notification_item_digest IS NOT NULL) AND NOT EXISTS (SELECT 1 FROM local_delivery_retries r WHERE r.item_id=router_pushes.push_id AND r.scheduled_retry_at IS NOT NULL) ORDER BY push_id",service,endpoint,session).fetch_all(&mut *connection).await?;
        decode_ids(rows, &mut connection).await
    }
    pub async fn pending_notification_listings(
        &self,
        target: &SessionRef,
    ) -> Result<Vec<PushRecord>, StorageError> {
        let mut connection = self.writer.lock().await;
        let service = String::from(target.endpoint.service_id.clone());
        let endpoint = String::from(target.endpoint.endpoint_id.clone());
        let session = String::from(target.session_id.clone());
        let rows=sqlx_turso::query_as!(NotificationPushIdRow,"SELECT push_id AS \"push_id!: String\" FROM router_pushes WHERE target_service_id=? AND target_endpoint_id=? AND target_session_id=? AND kind='notification-listing' AND delivery_state='pending' ORDER BY push_id",service,endpoint,session).fetch_all(&mut *connection).await?;
        decode_ids(rows, &mut connection).await
    }
    pub async fn freeze_notification_listing(
        &self,
        draft: PushRecordDraft,
        members: &[PushId],
    ) -> Result<PushRecord, StorageError> {
        let listing = draft
            .into_pending()
            .map_err(|_| StorageError::InvalidRecord)?;
        if listing.kind != PushKind::NotificationListing
            || members.is_empty()
            || members.iter().collect::<HashSet<_>>().len() != members.len()
        {
            return Err(StorageError::InvalidRecord);
        }
        if !matches!(listing.header_facts,collaboration_protocol::PushHeaderFacts::NotificationListing{item_count} if usize::try_from(item_count).ok()==Some(members.len()))
        {
            return Err(StorageError::InvalidRecord);
        }
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let changed=async {
            push_records::insert_push_record_with_connection(&mut transaction,&listing).await?;
            for id in members {
                let member=push_records::read_push_with_connection(&mut transaction,id).await?.ok_or(StorageError::PushNotFound)?;
                if !is_notification_kind(member.kind) || member.target!=listing.target || member.delivery_state!=PushDeliveryState::Held {return Err(StorageError::InvalidRecord);}
                let result=sqlx_turso::query!("UPDATE router_pushes SET notification_listing_push_id=? WHERE push_id=? AND notification_listing_push_id IS NULL AND delivery_state='held'",listing.push_id.as_str(),id.as_str()).execute(&mut *transaction).await?;
                if result.rows_affected()!=1 {return Err(StorageError::PushStateConflict);}
            }
            Ok(listing.clone())
        }.await;
        finish_listing_transaction(transaction, changed).await
    }
    pub async fn notification_listing_members(
        &self,
        caller: &SessionRef,
        listing_id: &PushId,
        after: Option<&PushId>,
        limit: u32,
    ) -> Result<(Vec<PushRecord>, Option<PushId>), StorageError> {
        if !(1..=u32::try_from(collaboration_protocol::limits::NOTIFICATION_LISTING_MAX_ENTRIES)
            .map_err(|_| StorageError::InvalidRecord)?)
            .contains(&limit)
        {
            return Err(StorageError::InvalidRecord);
        }
        let mut connection = self.writer.lock().await;
        let listing = push_records::read_push_with_connection(&mut connection, listing_id)
            .await?
            .ok_or(StorageError::PushNotFound)?;
        if listing.target != *caller {
            return Err(StorageError::PushNotPermitted);
        }
        if listing.kind != PushKind::NotificationListing {
            return Err(StorageError::InvalidRecord);
        }
        let after = after.map(PushId::as_str);
        let fetch_limit = i64::from(limit) + 1;
        let rows=sqlx_turso::query_as!(NotificationPushIdRow,"SELECT push_id AS \"push_id!: String\" FROM router_pushes WHERE notification_listing_push_id=? AND (? IS NULL OR push_id>?) ORDER BY push_id LIMIT ?",listing_id.as_str(),after,after,fetch_limit).fetch_all(&mut *connection).await?;
        let mut records = decode_ids(rows, &mut connection).await?;
        if records
            .iter()
            .any(|record| record.target != *caller || !is_notification_kind(record.kind))
        {
            return Err(StorageError::InvalidStoredRecord);
        }
        let has_more =
            records.len() > usize::try_from(limit).map_err(|_| StorageError::InvalidRecord)?;
        records.truncate(usize::try_from(limit).map_err(|_| StorageError::InvalidRecord)?);
        let next = if has_more {
            records.last().map(|record| record.push_id.clone())
        } else {
            None
        };
        Ok((records, next))
    }
}
pub(crate) async fn finish_listing_transaction<T>(
    transaction: Transaction<'_, Turso>,
    result: Result<T, StorageError>,
) -> Result<T, StorageError> {
    match result {
        Ok(value) => {
            transaction.commit().await?;
            Ok(value)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}
