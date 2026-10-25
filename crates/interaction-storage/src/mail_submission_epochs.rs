//! Board authority accompanies SDK custody without changing the immutable item.
use crate::{InteractionStore, StorageError, push_record_rows};
use chrono::{DateTime, Utc};
use collaboration_protocol::MailItemId;
use durable_steps::{Host, storage::TransactionScope, turso_store::TursoScope};
use message_board::{EpochNumber, ProjectId};
use sqlx_turso::TursoConnection;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailSubmissionEpoch {
    pub item_id: MailItemId,
    pub project: ProjectId,
    pub epoch: EpochNumber,
    pub recorded_at: DateTime<Utc>,
}

struct SubmissionEpochRow {
    item_id: String,
    project: String,
    epoch: i64,
    recorded_at: String,
}

impl SubmissionEpochRow {
    fn decode(self) -> Result<MailSubmissionEpoch, StorageError> {
        Ok(MailSubmissionEpoch {
            item_id: self
                .item_id
                .try_into()
                .map_err(|_| StorageError::InvalidStoredRecord)?,
            project: self
                .project
                .try_into()
                .map_err(|_| StorageError::InvalidStoredRecord)?,
            epoch: u64::try_from(self.epoch)
                .ok()
                .and_then(|epoch| EpochNumber::try_from(epoch).ok())
                .ok_or(StorageError::InvalidStoredRecord)?,
            recorded_at: push_record_rows::decode_timestamp(self.recorded_at)
                .map_err(|_| StorageError::InvalidStoredRecord)?,
        })
    }
}

async fn read_epoch(
    connection: &mut TursoConnection,
    item: &MailItemId,
) -> Result<Option<MailSubmissionEpoch>, StorageError> {
    sqlx_turso::query_as!(SubmissionEpochRow,
        "SELECT item_id AS \"item_id!: String\",project AS \"project!: String\",epoch AS \"epoch!: i64\",recorded_at AS \"recorded_at!: String\" FROM mail_submission_epochs WHERE item_id=?",
        item.as_str())
        .fetch_optional(connection).await?
        .map(SubmissionEpochRow::decode).transpose()
}

impl InteractionStore {
    pub async fn mail_submission_epoch(
        &self,
        item: &MailItemId,
    ) -> Result<Option<MailSubmissionEpoch>, StorageError> {
        read_epoch(&mut *self.writer.lock().await, item).await
    }
}

pub async fn read_mail_submission_epoch_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: &MailItemId,
) -> Result<Option<MailSubmissionEpoch>, StorageError> {
    read_epoch(scope.executor(), item).await
}

/// Record only Project mail, in the native transaction admitting its SDK Outbox row.
pub async fn record_mail_submission_epoch_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    item: &MailItemId,
    project: &ProjectId,
    epoch: EpochNumber,
    recorded_at: DateTime<Utc>,
) -> Result<(), StorageError> {
    if let Some(existing) = read_epoch(scope.executor(), item).await? {
        return if existing.project == *project && existing.epoch == epoch {
            Ok(())
        } else {
            Err(StorageError::InvalidStoredRecord)
        };
    }
    let epoch = i64::try_from(epoch.get()).map_err(|_| StorageError::InvalidRecord)?;
    let recorded_at = push_record_rows::serialize_timestamp(recorded_at);
    sqlx_turso::query!(
        "INSERT INTO mail_submission_epochs(item_id,project,epoch,recorded_at) VALUES (?,?,?,?)",
        item.as_str(),
        project.as_str(),
        epoch,
        recorded_at
    )
    .execute(scope.executor())
    .await?;
    Ok(())
}
