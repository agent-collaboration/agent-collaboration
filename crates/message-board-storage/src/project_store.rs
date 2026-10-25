//! Typed project access over the library-owned database and its guarded scopes.

use crate::{ProjectStorageError, StoreSchemaState, project_import_keys::RecordAdmission};
use fleet_sync_writer::{ReadTransaction, ReplicatedDatabase, ReplicatedSchema, WriteTransaction};
use message_board::{ActorOrigin, EpochNumber, FeedCursor, ProjectId};
use std::{path::Path, sync::Arc};

pub struct ProjectStore<TSchema: ReplicatedSchema> {
    pub(crate) project_id: ProjectId,
    pub(crate) database: Arc<ReplicatedDatabase<TSchema>>,
    schema: StoreSchemaState,
}

impl<TSchema: ReplicatedSchema> ProjectStore<TSchema> {
    pub async fn from_database(
        project_id: ProjectId,
        database: Arc<ReplicatedDatabase<TSchema>>,
    ) -> Result<Self, ProjectStorageError> {
        if database.database().to_string() != project_id.as_str() {
            return Err(ProjectStorageError::InvalidSchema);
        }
        let schema =
            crate::project_read_store::check_project_read_schema(database.reader()).await?;
        Ok(Self {
            project_id,
            database,
            schema,
        })
    }

    #[must_use]
    pub fn project_id(&self) -> &ProjectId {
        &self.project_id
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        self.database.path()
    }

    #[must_use]
    pub fn schema_state(&self) -> &StoreSchemaState {
        &self.schema
    }

    pub async fn begin_admission(
        &self,
        origin: ActorOrigin,
    ) -> Result<ProjectWriteTransaction, ProjectStorageError> {
        let mut transaction = self.database.begin_write().await?;
        if let StoreSchemaState::ReadOnly { refusal } = &self.schema {
            transaction.rollback().await?;
            return Err(ProjectStorageError::SchemaNotReady(refusal.clone()));
        }
        // The native writer guard prevents publication while its receipt and the
        // holder-local baseline are checked. The Host alone creates the baseline.
        if let Some(receipt) = self.database.installed_snapshot().await?
            && let Err(error) =
                crate::project_capture_marks::require_snapshot_capture(&mut transaction, &receipt)
                    .await
        {
            transaction.rollback().await?;
            return Err(error);
        }
        ProjectWriteTransaction::live(self.project_id.clone(), transaction, origin).await
    }

    pub async fn writer_state(&self) -> Result<fleet_sync_lease::WriterState, ProjectStorageError> {
        Ok(self.database.writer_state().await?)
    }

    pub async fn registration_outcome(
        &self,
    ) -> Result<fleet_sync_writer::WriterRegistrationOutcome, ProjectStorageError> {
        Ok(self.database.registration_outcome().await?)
    }

    #[must_use]
    pub fn reader(&self) -> crate::ProjectReadStore<'_> {
        crate::ProjectReadStore::new(&self.project_id, self.database.reader(), &self.schema)
    }

    pub async fn snapshot(&self) -> Result<ProjectSnapshot, ProjectStorageError> {
        self.reader().snapshot().await
    }

    pub async fn has_thread(
        &self,
        thread: &message_board::ThreadId,
    ) -> Result<bool, ProjectStorageError> {
        self.reader().has_thread(thread).await
    }

    pub async fn has_post(
        &self,
        post: &message_board::PostId,
    ) -> Result<bool, ProjectStorageError> {
        self.reader().has_post(post).await
    }

    pub async fn has_subscription_memberships(
        &self,
        reader: Option<&message_board::Identity>,
    ) -> Result<bool, ProjectStorageError> {
        self.reader().has_subscription_memberships(reader).await
    }

    pub async fn has_active_inbox_watch(
        &self,
        reader: &message_board::Identity,
    ) -> Result<bool, ProjectStorageError> {
        self.reader().has_active_inbox_watch(reader).await
    }

    pub async fn close(self) {
        self.database.close().await;
    }

    pub async fn close_connections(&self) {
        self.database.close().await;
    }
}

pub struct ProjectWriteTransaction {
    pub(crate) transaction: WriteTransaction,
    pub(crate) project_id: ProjectId,
    pub(crate) admission: RecordAdmission,
}

impl ProjectWriteTransaction {
    pub fn origin(&self) -> ActorOrigin {
        self.admission.origin()
    }
    pub fn record_placement(&self, cursor: FeedCursor) -> message_board::RecordPlacement {
        message_board::RecordPlacement::Project(message_board::ProjectFeedEntry {
            cursor,
            origin: self.admission.origin(),
        })
    }
    pub async fn live(
        project_id: ProjectId,
        transaction: WriteTransaction,
        origin: ActorOrigin,
    ) -> Result<Self, ProjectStorageError> {
        Self::admitted(project_id, transaction, RecordAdmission::Live { origin }).await
    }

    pub async fn for_import(
        project_id: ProjectId,
        transaction: WriteTransaction,
        provenance: fleet_admission::ImportProvenance,
        row_keys: Arc<dyn crate::ImportRowKeys>,
    ) -> Result<Self, ProjectStorageError> {
        Self::admitted(
            project_id,
            transaction,
            RecordAdmission::Import {
                provenance,
                row_keys,
            },
        )
        .await
    }

    async fn admitted(
        project_id: ProjectId,
        mut transaction: WriteTransaction,
        admission: RecordAdmission,
    ) -> Result<Self, ProjectStorageError> {
        crate::project_epoch_reads::ensure_epoch_line(&mut transaction, &admission).await?;
        Ok(Self {
            transaction,
            project_id,
            admission,
        })
    }

    pub fn admitted_epoch(&self) -> Result<EpochNumber, ProjectStorageError> {
        EpochNumber::try_from(self.transaction.admitted_epoch().value())
            .map_err(|_| ProjectStorageError::InvalidFeed)
    }

    pub async fn high_water(&mut self) -> Result<FeedCursor, ProjectStorageError> {
        checkpoint(&mut self.transaction).await
    }

    pub async fn commit(self) -> Result<(), ProjectStorageError> {
        self.transaction.commit().await?;
        Ok(())
    }

    pub async fn rollback(self) -> Result<(), ProjectStorageError> {
        self.transaction.rollback().await?;
        Ok(())
    }
}

pub struct ProjectSnapshot {
    pub(crate) transaction: ReadTransaction,
    pub(crate) high_water: FeedCursor,
    pub(crate) project_id: ProjectId,
}

impl ProjectSnapshot {
    #[must_use]
    pub fn high_water(&self) -> FeedCursor {
        self.high_water
    }

    pub async fn finish(self) -> Result<(), ProjectStorageError> {
        self.transaction.close().await?;
        Ok(())
    }
}

pub(crate) async fn checkpoint<TScope>(
    scope: &mut TScope,
) -> Result<FeedCursor, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: sqlx::Executor<'scope, Database = sqlx_turso::Turso>,
{
    crate::project_epoch_reads::checkpoint(scope).await
}

#[cfg(test)]
#[path = "project_bookkeeping_tests.rs"]
mod bookkeeping_tests;
