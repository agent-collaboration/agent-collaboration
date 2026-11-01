//! Immutable SDK references permit policy recovery without reading SDK-owned tables.
use crate::{InteractionStore, StorageError, push_records};
use collaboration_protocol::{PushId, PushKind, PushRecord};
use durable_steps::{Host, storage::TransactionScope, turso_store::TursoScope};
use fleet_sync_protocol::{DeliveryItemRef, ItemDigest, ItemId};
use sqlx_turso::TursoConnection;

struct NotificationReferenceRow {
    notification_item_digest: Option<String>,
}
struct ListingMemberRow {
    push_id: String,
}
#[derive(Clone, Debug)]
pub struct NotificationListingMember {
    pub record: PushRecord,
    pub reference: Option<DeliveryItemRef>,
}
pub(crate) async fn notification_reference(
    connection: &mut TursoConnection,
    id: &PushId,
) -> Result<Option<DeliveryItemRef>, StorageError> {
    let row = sqlx_turso::query_as!(
        NotificationReferenceRow,
        "SELECT notification_item_digest FROM router_pushes WHERE push_id=?",
        id.as_str()
    )
    .fetch_optional(connection)
    .await?
    .ok_or(StorageError::PushNotFound)?;
    row.notification_item_digest
        .map(|digest| {
            Ok(DeliveryItemRef {
                item: ItemId::try_from(id.as_str().to_owned())
                    .map_err(|_| StorageError::InvalidStoredRecord)?,
                digest: ItemDigest::try_from(digest)
                    .map_err(|_| StorageError::InvalidStoredRecord)?,
            })
        })
        .transpose()
}
pub async fn record_notification_reference_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    reference: &DeliveryItemRef,
) -> Result<(), StorageError> {
    let id = PushId::try_from(reference.item.as_uuid().to_string())
        .map_err(|_| StorageError::InvalidRecord)?;
    let Some(record) = push_records::read_push_with_connection(scope.executor(), &id).await? else {
        return Err(StorageError::PushNotFound);
    };
    if !crate::notification_listings::is_notification_kind(record.kind) {
        return Err(StorageError::InvalidRecord);
    }
    if let Some(existing) = notification_reference(scope.executor(), &id).await? {
        return if existing == *reference {
            Ok(())
        } else {
            Err(StorageError::PushContentConflict)
        };
    }
    sqlx_turso::query!("UPDATE router_pushes SET notification_item_digest=? WHERE push_id=? AND notification_item_digest IS NULL",reference.digest.as_str(),id.as_str()).execute(scope.executor()).await?;
    Ok(())
}
pub async fn notification_listing_for_member_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    id: &PushId,
) -> Result<Option<PushRecord>, StorageError> {
    crate::notification_listings::listing_for_member(scope.executor(), id).await
}
/// A known-unsent listing keeps SDK membership until that item's real callback commits.
pub async fn release_unsent_notification_member_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    id: &PushId,
) -> Result<(), StorageError> {
    if let Some(listing) =
        crate::notification_listings::listing_for_member(scope.executor(), id).await?
        && listing.delivery_state == collaboration_protocol::PushDeliveryState::Rejected
    {
        sqlx_turso::query!("UPDATE router_pushes SET notification_listing_push_id=NULL WHERE push_id=? AND notification_listing_push_id=?",id.as_str(),listing.push_id.as_str()).execute(scope.executor()).await?;
    }
    Ok(())
}
pub async fn prepare_notification_listing_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    id: &PushId,
) -> Result<Vec<NotificationListingMember>, StorageError> {
    let listing = push_records::read_push_with_connection(scope.executor(), id)
        .await?
        .ok_or(StorageError::PushNotFound)?;
    if listing.kind != PushKind::NotificationListing
        || listing.delivery_state != collaboration_protocol::PushDeliveryState::Pending
    {
        return Err(StorageError::PushStateConflict);
    }
    let rows=sqlx_turso::query_as!(ListingMemberRow,"SELECT push_id AS \"push_id!: String\" FROM router_pushes WHERE notification_listing_push_id=? ORDER BY push_id",id.as_str()).fetch_all(scope.executor()).await?;
    if rows.is_empty() {
        return Err(StorageError::InvalidStoredRecord);
    }
    let mut members = Vec::with_capacity(rows.len());
    for row in rows {
        let member_id =
            PushId::try_from(row.push_id).map_err(|_| StorageError::InvalidStoredRecord)?;
        let record = push_records::read_push_with_connection(scope.executor(), &member_id)
            .await?
            .ok_or(StorageError::PushNotFound)?;
        if record.target != listing.target
            || !crate::notification_listings::is_notification_kind(record.kind)
            || record.delivery_state != collaboration_protocol::PushDeliveryState::Held
        {
            return Err(StorageError::PushStateConflict);
        }
        let reference = notification_reference(scope.executor(), &member_id).await?;
        // Carrier members stay Held under the immutable listing reference. Only
        // the listing performs native I/O, so a known-unsent listing can unlink
        // them without inventing an attempt for each original notification.
        members.push(NotificationListingMember { record, reference });
    }
    let changed=sqlx_turso::query!("UPDATE router_pushes SET delivery_state='attempted',last_outcome_json=NULL,settled_at=NULL WHERE push_id=? AND delivery_state='pending' AND kind='notification-listing'",id.as_str()).execute(scope.executor()).await?;
    if changed.rows_affected() != 1 {
        return Err(StorageError::PushStateConflict);
    }
    Ok(members)
}
impl InteractionStore {
    pub async fn notification_item_reference(
        &self,
        id: &PushId,
    ) -> Result<Option<DeliveryItemRef>, StorageError> {
        notification_reference(&mut *self.writer.lock().await, id).await
    }
}
