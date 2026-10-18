//! Router-local messaging authority. Carrier changes use this same transaction.
use crate::{AutomationStore, StorageError, mail_custody_records as records};
use chrono::{DateTime, Utc};
use collaboration_mail_protocol::{
    MailBody, MailDigest, MailItem, MailItemContent, MailItemId, MailSender, MailStatus,
    MessageBody, StatusVersion,
};
use collaboration_protocol::{
    MachineId, PushId, PushKind, PushMessageSendResult, PushOrigin, PushRecordDraft, RouterLink,
};
use serde::{Deserialize, Serialize};
use sqlx::Connection;

pub struct LocalMessageRequest {
    pub draft: PushRecordDraft,
    pub target_identity: String,
}
#[derive(Debug)]
pub enum LocalMessageAdmission {
    Pending(PushId),
    Replayed(Box<PushMessageSendResult>),
}
#[derive(Clone, Debug)]
pub struct LocalMailSnapshot {
    pub item_id: MailItemId,
    pub digest: MailDigest,
    pub status: MailStatus,
    pub version: StatusVersion,
    pub terminal_at: Option<DateTime<Utc>>,
    pub compacted: bool,
    pub result: Option<PushMessageSendResult>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LocalMessageProjection {
    pub target_identity: String,
}

impl AutomationStore {
    pub async fn prepare_local_message(
        &mut self,
        request: &LocalMessageRequest,
    ) -> Result<LocalMessageAdmission, StorageError> {
        let record = request
            .draft
            .clone()
            .into_pending()
            .map_err(|_| StorageError::InvalidRecord)?;
        let item = local_item(&request.draft)?;
        if request.target_identity.is_empty() {
            return Err(StorageError::InvalidRecord);
        }
        let mut transaction = self.connection.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(admission) = known_local_message(&mut transaction, &item).await? {
            transaction.commit().await?;
            return Ok(admission);
        }
        records::reserve_capacity(&mut transaction, &item).await?;
        records::insert(
            &mut transaction,
            &item,
            &LocalMessageProjection {
                target_identity: request.target_identity.clone(),
            },
            record.created_at,
        )
        .await?;
        crate::push_records::insert_push_record_with_retention(
            &mut transaction,
            &record,
            crate::push_retention::PushRetention::MailTerminalAge,
        )
        .await?;
        transaction.commit().await?;
        Ok(LocalMessageAdmission::Pending(record.push_id))
    }
    /// Answers known identities without requiring a native delivery service.
    pub async fn replay_local_message(
        &mut self,
        request: &LocalMessageRequest,
    ) -> Result<Option<LocalMessageAdmission>, StorageError> {
        request
            .draft
            .clone()
            .into_pending()
            .map_err(|_| StorageError::InvalidRecord)?;
        let item = local_item(&request.draft)?;
        known_local_message(&mut self.connection, &item).await
    }
    pub async fn read_local_mail(
        &mut self,
        id: &MailItemId,
    ) -> Result<Option<LocalMailSnapshot>, StorageError> {
        Ok(records::read(&mut self.connection, id)
            .await?
            .map(|row| row.snapshot))
    }
}

pub(crate) fn local_item(draft: &PushRecordDraft) -> Result<MailItem, StorageError> {
    let invalid = || StorageError::InvalidRecord;
    if draft.kind != PushKind::DirectMessage {
        return Err(invalid());
    }
    let recipient: message_board::SessionRef =
        draft.target.clone().try_into().map_err(|_| invalid())?;
    let sender = match &draft.origin {
        PushOrigin::Session(sender)
            if sender.endpoint.service_id == draft.target.endpoint.service_id =>
        {
            MailSender::Session(sender.clone().try_into().map_err(|_| invalid())?)
        }
        PushOrigin::OwnerUnverified => MailSender::HumanUser {
            installation: recipient.endpoint.service_id.clone(),
        },
        _ => return Err(invalid()),
    };
    MailItem::new(MailItemContent {
        id: draft.push_id.clone().try_into().map_err(|_| invalid())?,
        sender,
        recipient,
        valid_until: None,
        body: MailBody::Message(MessageBody {
            text: draft
                .body
                .clone()
                .ok_or_else(invalid)?
                .try_into()
                .map_err(|_| invalid())?,
            delivery: draft.mode.ok_or_else(invalid)?.into(),
            guard: draft
                .guard
                .clone()
                .map(TryInto::try_into)
                .transpose()
                .map_err(|_| invalid())?,
        }),
    })
    .map_err(|_| invalid())
}

pub(crate) fn result_for(
    record: &collaboration_protocol::PushRecord,
    projection: &LocalMessageProjection,
) -> Result<PushMessageSendResult, StorageError> {
    Ok(PushMessageSendResult {
        push_id: record.push_id.clone(),
        link: RouterLink::new(
            MachineId::from(record.target.endpoint.service_id.clone()),
            record.push_id.clone(),
        )
        .to_string(),
        target: record.target.clone(),
        target_identity: projection.target_identity.clone(),
        delivery_state: record.delivery_state,
        receipt: record
            .last_outcome
            .clone()
            .ok_or(StorageError::InvalidRecord)?,
    })
}

async fn known_local_message(
    connection: &mut sqlx::SqliteConnection,
    item: &MailItem,
) -> Result<Option<LocalMessageAdmission>, StorageError> {
    let Some(existing) = records::read(connection, item.id()).await? else {
        return Ok(None);
    };
    if existing.snapshot.digest != *item.digest() {
        return Err(StorageError::MailRefused(
            collaboration_mail_protocol::MailRefusal::ItemConflict {
                existing: existing.snapshot.digest,
            },
        ));
    }
    Ok(Some(match existing.snapshot.result {
        Some(result) => LocalMessageAdmission::Replayed(Box::new(result)),
        None => LocalMessageAdmission::Pending(
            item.id()
                .clone()
                .try_into()
                .map_err(|_| StorageError::InvalidRecord)?,
        ),
    }))
}
