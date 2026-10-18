//! One admission writer and one maintenance/read connection, with no public raw handle.

use message_board::{EpochNumber, FeedCursor, FeedPosition, ProjectId};
use sqlx::{Pool, Transaction, pool::PoolOptions};
use sqlx_turso::{Turso, TursoConnectOptions};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::{ProjectStorageError, StoreSchemaState};
#[path = "project_store_move.rs"]
mod project_store_move;
use project_store_move::ProjectReadAccess;
pub(crate) use project_store_move::verify_move_integrity;

pub struct ProjectStore {
    project_id: ProjectId,
    path: PathBuf,
    authoritative: Pool<Turso>,
    maintenance: Pool<Turso>,
    writer_mutex: Arc<Mutex<()>>,
    schema: StoreSchemaState,
    read_access: ProjectReadAccess,
}

impl ProjectStore {
    pub async fn open(path: &Path, project_id: ProjectId) -> Result<Self, ProjectStorageError> {
        tracing::trace!(target: "message_board_storage::open", project = %project_id.as_str(), "project store open");
        let authoritative = open_project_pool(path, CaptureMode::Authoritative).await?;
        let schema = {
            let mut connection = authoritative.acquire().await?;
            crate::project_schema::migrate_project_schema(&mut connection).await?
        };
        let maintenance = open_project_pool(path, CaptureMode::Maintenance).await?;
        if matches!(schema, StoreSchemaState::Ready { .. }) {
            let mut connection = maintenance.acquire().await?;
            crate::project_bookkeeping::migrate_bookkeeping(&mut connection).await?;
        }
        Ok(Self {
            project_id,
            path: path.to_owned(),
            authoritative,
            maintenance,
            writer_mutex: Arc::new(Mutex::new(())),
            schema,
            read_access: ProjectReadAccess::new(),
        })
    }
    #[must_use]
    pub fn project_id(&self) -> &ProjectId {
        &self.project_id
    }
    #[must_use]
    pub fn is_open(&self) -> bool {
        !self.maintenance.is_closed()
    }
    #[tracing::instrument(skip_all, fields(project = %self.project_id.as_str()))]
    pub async fn has_thread(
        &self,
        thread: &message_board::ThreadId,
    ) -> Result<bool, ProjectStorageError> {
        let _access = self.read_access.enter().await?;
        let thread = thread.as_str();
        Ok(sqlx_turso::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM threads WHERE thread_id = ?) AS "present!: bool""#,
            thread
        )
        .fetch_one(&self.maintenance)
        .await?)
    }
    pub async fn has_post(
        &self,
        post: &message_board::PostId,
    ) -> Result<bool, ProjectStorageError> {
        let _access = self.read_access.enter().await?;
        let post = post.as_str();
        Ok(sqlx_turso::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM posts WHERE post_id = ?) AS "present!: bool""#,
            post
        )
        .fetch_one(&self.maintenance)
        .await?)
    }
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
    #[must_use]
    pub fn schema_state(&self) -> &StoreSchemaState {
        &self.schema
    }

    pub async fn begin_admission(&self) -> Result<ProjectWriteTransaction, ProjectStorageError> {
        self.lock_writer().await.begin().await
    }

    /// Admission checks its local lease while holding this guard, including on read-only stores.
    pub async fn lock_writer(&self) -> ProjectWriterGuard {
        ProjectWriterGuard {
            writer_guard: self.writer_mutex.clone().lock_owned().await,
            authoritative: self.authoritative.clone(),
            project_id: self.project_id.clone(),
            schema: self.schema.clone(),
            read_access: self.read_access.clone(),
        }
    }

    #[tracing::instrument(skip_all, fields(project = %self.project_id.as_str()))]
    pub async fn snapshot(&self) -> Result<ProjectSnapshot, ProjectStorageError> {
        let access_guard = self.read_access.enter().await?;
        let mut transaction = self.maintenance.begin().await?;
        let cursor = checkpoint(&mut transaction).await?;
        Ok(ProjectSnapshot {
            transaction,
            high_water: cursor,
            project_id: self.project_id.clone(),
            _access_guard: Some(access_guard),
        })
    }
    #[cfg(test)]
    pub(crate) async fn recycle_authoritative_connection(&self) -> Result<(), ProjectStorageError> {
        let _writer_guard = self.writer_mutex.lock().await;
        self.authoritative.acquire().await?.close().await?;
        Ok(())
    }
    pub async fn close(self) {
        self.close_connections().await;
    }
    pub async fn close_connections(&self) {
        self.authoritative.close().await;
        self.maintenance.close().await;
    }
}

/// Owns the same store mutex before a schema refusal or a write transaction is selected.
pub struct ProjectWriterGuard {
    writer_guard: OwnedMutexGuard<()>,
    authoritative: Pool<Turso>,
    project_id: ProjectId,
    schema: StoreSchemaState,
    read_access: ProjectReadAccess,
}
impl ProjectWriterGuard {
    #[must_use]
    pub fn schema_state(&self) -> &StoreSchemaState {
        &self.schema
    }
    pub async fn begin(self) -> Result<ProjectWriteTransaction, ProjectStorageError> {
        self.read_access.require_open()?;
        if let StoreSchemaState::ReadOnly { refusal } = &self.schema {
            return Err(ProjectStorageError::SchemaNotReady(refusal.clone()));
        }
        let transaction = self.authoritative.begin_with("BEGIN IMMEDIATE").await?;
        Ok(ProjectWriteTransaction {
            transaction,
            writer_guard: self.writer_guard,
            project_id: self.project_id,
        })
    }
}

#[derive(Clone, Copy)]
enum CaptureMode {
    Authoritative,
    Maintenance,
}

async fn open_project_pool(
    path: &Path,
    mode: CaptureMode,
) -> Result<Pool<Turso>, ProjectStorageError> {
    let options = TursoConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    Ok(PoolOptions::<Turso>::new()
        .max_connections(1)
        .min_connections(1)
        .after_connect(move |connection, _| {
            Box::pin(async move {
                let sql = match mode {
                    CaptureMode::Authoritative => "PRAGMA capture_data_changes_conn('full')",
                    CaptureMode::Maintenance => "PRAGMA capture_data_changes_conn('off')",
                };
                sqlx::query(sql).execute(connection).await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await?)
}

pub struct ProjectWriteTransaction {
    pub(crate) transaction: Transaction<'static, Turso>,
    writer_guard: OwnedMutexGuard<()>,
    pub(crate) project_id: ProjectId,
}
/// Keeps the project mutex through a post-commit file transition.
pub struct ProjectCommitGuard {
    _writer_guard: OwnedMutexGuard<()>,
    project_id: ProjectId,
}
impl ProjectCommitGuard {
    #[must_use]
    pub fn project_id(&self) -> &ProjectId {
        &self.project_id
    }
}
impl ProjectWriteTransaction {
    pub async fn high_water(&mut self) -> Result<FeedCursor, ProjectStorageError> {
        checkpoint(&mut self.transaction).await
    }
    pub async fn commit(self) -> Result<(), ProjectStorageError> {
        self.transaction.commit().await?;
        drop(self.writer_guard);
        Ok(())
    }
    pub async fn commit_retaining_writer(self) -> Result<ProjectCommitGuard, ProjectStorageError> {
        self.transaction.commit().await?;
        Ok(ProjectCommitGuard {
            _writer_guard: self.writer_guard,
            project_id: self.project_id,
        })
    }
    pub async fn rollback(self) -> Result<(), ProjectStorageError> {
        self.transaction.rollback().await?;
        drop(self.writer_guard);
        Ok(())
    }
}

pub struct ProjectSnapshot {
    pub(crate) transaction: Transaction<'static, Turso>,
    pub(crate) high_water: FeedCursor,
    pub(crate) project_id: ProjectId,
    pub(crate) _access_guard: Option<tokio::sync::OwnedRwLockReadGuard<()>>,
}
impl ProjectSnapshot {
    #[must_use]
    pub fn high_water(&self) -> FeedCursor {
        self.high_water
    }
    pub async fn finish(self) -> Result<(), ProjectStorageError> {
        self.transaction.commit().await?;
        Ok(())
    }
}

pub(crate) async fn checkpoint(
    connection: &mut sqlx_turso::TursoConnection,
) -> Result<FeedCursor, ProjectStorageError> {
    let row = sqlx_turso::query!(r#"SELECT epoch AS "epoch!: i64", last_position AS "last_position!: i64", (SELECT COUNT(*) FROM feed_checkpoint) AS "count!: i64" FROM feed_checkpoint WHERE singleton = 1"#).fetch_one(connection).await?;
    if row.count != 1 {
        return Err(ProjectStorageError::InvalidFeed);
    }
    let epoch = u64::try_from(row.epoch)
        .ok()
        .and_then(|epoch| EpochNumber::try_from(epoch).ok())
        .ok_or(ProjectStorageError::InvalidFeed)?;
    let position = u64::try_from(row.last_position)
        .ok()
        .and_then(|position| FeedPosition::try_from(position).ok())
        .ok_or(ProjectStorageError::InvalidFeed)?;
    Ok(FeedCursor { epoch, position })
}

#[cfg(test)]
#[path = "project_bookkeeping_tests.rs"]
mod bookkeeping_tests;

impl ProjectStore {
    #[tracing::instrument(skip_all, fields(project = %self.project_id().as_str()))]
    pub async fn has_subscription_memberships(
        &self,
        reader: Option<&message_board::Identity>,
    ) -> Result<bool, ProjectStorageError> {
        let _access = self.read_access.enter().await?;
        let found = match reader {
            Some(reader) => {
                let reader = serde_json::to_string(reader)?;
                sqlx_turso::query_scalar!(r#"SELECT membership_id AS "membership_id!: String" FROM thread_subscriptions WHERE reader = ? LIMIT 1"#, reader).fetch_optional(&self.maintenance).await?
            },
            None => sqlx_turso::query_scalar!(r#"SELECT membership_id AS "membership_id!: String" FROM thread_subscriptions LIMIT 1"#).fetch_optional(&self.maintenance).await?,
        };
        Ok(found.is_some())
    }

    #[tracing::instrument(skip_all, fields(project = %self.project_id().as_str()))]
    pub async fn has_active_inbox_watch(
        &self,
        reader: &message_board::Identity,
    ) -> Result<bool, ProjectStorageError> {
        let _access = self.read_access.enter().await?;
        let encoded = serde_json::to_string(reader)?;
        let reader = encoded.as_str();
        // EXISTS answers scope; the selected raw flags retain Rust validation
        // when an offline fault bypassed the boolean storage constraints.
        let row = sqlx_turso::query!(r#"SELECT EXISTS(SELECT 1 FROM thread_watches WHERE reader = ? AND active = 1) OR EXISTS(SELECT 1 FROM project_watches WHERE reader = ? AND active = 1) AS "active!: bool", (SELECT active FROM thread_watches WHERE reader = ? AND active != 0 LIMIT 1) AS "thread_active: i64", (SELECT active FROM project_watches WHERE reader = ? AND active != 0 LIMIT 1) AS "project_active: i64""#, reader, reader, reader, reader)
            .fetch_one(&self.maintenance).await?;
        match (row.thread_active, row.project_active) {
            (None | Some(1), None | Some(1)) => Ok(row.active),
            _ => Err(ProjectStorageError::InvalidFeed),
        }
    }
}
