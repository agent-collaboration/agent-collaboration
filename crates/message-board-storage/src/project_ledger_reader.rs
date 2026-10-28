//! Orphaned files expose only ledger lookups, with no migration or writer capability.
use crate::{ProjectSnapshot, ProjectStorageError};
use fleet_sync_writer::{ReplicatedSchema, RetainedFile, WriterRegistrationOutcome};
use message_board::{
    FeedCursor, ProjectId, ProjectRegistrationRejection, Record, RequestId, RequestLedgerEntry,
};
use std::path::Path;
use tokio::sync::Mutex;

pub struct ProjectLedgerReader {
    readers: Mutex<Option<RetainedFile>>,
    project: ProjectId,
    registration: WriterRegistrationOutcome,
}
impl ProjectLedgerReader {
    pub async fn open<TSchema: ReplicatedSchema>(
        path: &Path,
        project: ProjectId,
        schema: TSchema,
    ) -> Result<Self, ProjectStorageError> {
        tracing::trace!(target: "message_board_storage::open", project = %project.as_str(), "retained project ledger open");
        let retained = RetainedFile::open_read_only(path, schema).await?;
        let registration = retained.registration_outcome().clone();
        Ok(Self {
            readers: Mutex::new(Some(retained)),
            project,
            registration,
        })
    }
    async fn snapshot(&self) -> Result<ProjectSnapshot, ProjectStorageError> {
        let readers = self.readers.lock().await;
        let readers = readers
            .as_ref()
            .ok_or(ProjectStorageError::RetainedFileUnavailable)?;
        let mut transaction = readers.reader().begin_read().await?;
        let high_water = crate::project_store::checkpoint(&mut transaction).await?;
        Ok(ProjectSnapshot {
            transaction,
            high_water,
            project_id: self.project.clone(),
        })
    }
    pub async fn ledger(
        &self,
        id: &RequestId,
    ) -> Result<Option<RequestLedgerEntry>, ProjectStorageError> {
        let mut snapshot = self.snapshot().await?;
        let record = snapshot.record(&id.clone().into()).await?;
        snapshot.finish().await?;
        match record {
            Some(Record::Request(ledger)) => Ok(Some(ledger)),
            None => Ok(None),
            _ => Err(ProjectStorageError::InvalidFeed),
        }
    }
    pub fn registration_outcome(&self) -> &WriterRegistrationOutcome {
        &self.registration
    }
    pub fn rejection_reason(&self) -> Option<ProjectRegistrationRejection> {
        match &self.registration {
            WriterRegistrationOutcome::Refused {
                reason: fleet_sync_protocol::RegistrationRefusal::DatabaseRejected { reason },
            } => Some(ProjectRegistrationRejection {
                code: reason.code.as_str().to_owned(),
                detail: reason.detail.clone(),
            }),
            _ => None,
        }
    }
    pub async fn creating_request(&self) -> Result<Option<RequestId>, ProjectStorageError> {
        let mut snapshot = self.snapshot().await?;
        let request = snapshot.project_creation_request().await?;
        snapshot.finish().await?;
        Ok(request)
    }
    pub async fn high_water(&self) -> Result<FeedCursor, ProjectStorageError> {
        let snapshot = self.snapshot().await?;
        let high_water = snapshot.high_water();
        snapshot.finish().await?;
        Ok(high_water)
    }
    pub async fn close(&self) {
        self.readers.lock().await.take();
    }
}
