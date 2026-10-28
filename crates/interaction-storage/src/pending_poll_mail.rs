//! Poll policy facts discover SDK-owned items without creating a delivery push.
use crate::{InteractionStore, StorageError};
use chrono::{DateTime, SecondsFormat, Utc};
use collaboration_protocol::SessionRef;
use durable_steps::{Host, storage::TransactionScope, turso_store::TursoScope};
use fleet_sync_protocol::ItemId;
use message_board::RecordId;
use sqlx_turso::TursoConnection;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingPollMail {
    pub item: ItemId,
    pub recipient: SessionRef,
    pub membership: RecordId,
    pub received_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct PendingPollRow {
    item_id: String,
    recipient_service_id: String,
    recipient_endpoint_id: String,
    recipient_session_id: String,
    membership_id: String,
    received_at: String,
}

impl TryFrom<PendingPollRow> for PendingPollMail {
    type Error = StorageError;
    fn try_from(row: PendingPollRow) -> Result<Self, Self::Error> {
        let invalid = || StorageError::InvalidStoredRecord;
        let received_at = DateTime::parse_from_rfc3339(&row.received_at)
            .map_err(|_| invalid())?
            .with_timezone(&Utc);
        if received_at.to_rfc3339_opts(SecondsFormat::Nanos, true) != row.received_at {
            return Err(invalid());
        }
        Ok(Self {
            item: row.item_id.try_into().map_err(|_| invalid())?,
            recipient: SessionRef {
                endpoint: collaboration_protocol::EndpointRef {
                    service_id: row.recipient_service_id.try_into().map_err(|_| invalid())?,
                    endpoint_id: row
                        .recipient_endpoint_id
                        .try_into()
                        .map_err(|_| invalid())?,
                },
                session_id: row.recipient_session_id.try_into().map_err(|_| invalid())?,
            },
            membership: row.membership_id.try_into().map_err(|_| invalid())?,
            received_at,
        })
    }
}

async fn read_pending_poll(
    connection: &mut TursoConnection,
    item: ItemId,
) -> Result<Option<PendingPollMail>, StorageError> {
    let id = item.as_uuid().to_string();
    sqlx_turso::query_as!(PendingPollRow,
        r#"SELECT item_id AS "item_id!",recipient_service_id AS "recipient_service_id!",recipient_endpoint_id AS "recipient_endpoint_id!",recipient_session_id AS "recipient_session_id!",membership_id AS "membership_id!",received_at AS "received_at!" FROM pending_poll_mail WHERE item_id=?"#, id
    ).fetch_optional(connection).await?.map(TryInto::try_into).transpose()
}

pub async fn insert_pending_poll_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    policy: &PendingPollMail,
) -> Result<(), StorageError> {
    let item = policy.item.as_uuid().to_string();
    let service = String::from(policy.recipient.endpoint.service_id.clone());
    let endpoint = String::from(policy.recipient.endpoint.endpoint_id.clone());
    let session = String::from(policy.recipient.session_id.clone());
    let at = policy
        .received_at
        .to_rfc3339_opts(SecondsFormat::Nanos, true);
    sqlx_turso::query!(
        "INSERT INTO pending_poll_mail(item_id,recipient_service_id,recipient_endpoint_id,recipient_session_id,membership_id,received_at) VALUES(?,?,?,?,?,?) ON CONFLICT(item_id) DO NOTHING",
        item,service,endpoint,session,policy.membership.as_str(),at
    ).execute(scope.executor()).await?;
    if read_pending_poll(scope.executor(), policy.item)
        .await?
        .as_ref()
        != Some(policy)
    {
        return Err(StorageError::InvalidStoredRecord);
    }
    Ok(())
}

pub async fn read_pending_poll_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: ItemId,
) -> Result<Option<PendingPollMail>, StorageError> {
    read_pending_poll(scope.executor(), item).await
}

pub async fn delete_pending_poll_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: ItemId,
) -> Result<(), StorageError> {
    let id = item.as_uuid().to_string();
    sqlx_turso::query!("DELETE FROM pending_poll_mail WHERE item_id=?", id)
        .execute(scope.executor())
        .await?;
    Ok(())
}

impl InteractionStore {
    pub async fn pending_poll_mail(
        &self,
        recipient: &SessionRef,
    ) -> Result<Vec<PendingPollMail>, StorageError> {
        let service = String::from(recipient.endpoint.service_id.clone());
        let endpoint = String::from(recipient.endpoint.endpoint_id.clone());
        let session = String::from(recipient.session_id.clone());
        sqlx_turso::query_file_as!(
            PendingPollRow,
            "src/pending_poll_mail.sql",
            service,
            endpoint,
            session
        )
        .fetch_all(&mut *self.writer.lock().await)
        .await?
        .into_iter()
        .map(TryInto::try_into)
        .collect()
    }

    pub async fn pending_poll_item(
        &self,
        item: ItemId,
    ) -> Result<Option<PendingPollMail>, StorageError> {
        read_pending_poll(&mut *self.writer.lock().await, item).await
    }
}
