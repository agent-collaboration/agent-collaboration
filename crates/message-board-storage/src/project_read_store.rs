//! Shared read capability over either a writable owner's or a retained file's guarded pool.
use crate::{ProjectSnapshot, ProjectStorageError, StoreSchemaState};
use fleet_sync_writer::ReadPool;
use message_board::ProjectId;

pub(crate) async fn check_project_read_schema(
    pool: &ReadPool,
) -> Result<StoreSchemaState, ProjectStorageError> {
    let mut transaction = pool.begin_read().await?;
    let schema = crate::check_project_schema(&mut transaction).await;
    transaction.close().await?;
    schema
}

pub struct ProjectReadStore<'store> {
    project_id: &'store ProjectId,
    pool: &'store ReadPool,
    schema: &'store StoreSchemaState,
}

impl<'store> ProjectReadStore<'store> {
    pub(crate) fn new(
        project_id: &'store ProjectId,
        pool: &'store ReadPool,
        schema: &'store StoreSchemaState,
    ) -> Self {
        Self {
            project_id,
            pool,
            schema,
        }
    }

    #[must_use]
    pub fn schema_state(&self) -> &StoreSchemaState {
        self.schema
    }

    pub async fn acknowledged_capture(
        &self,
        through: fleet_sync_protocol::ReplicationPosition,
    ) -> Result<crate::AcknowledgedProjectChanges, ProjectStorageError> {
        let mut read = self.pool.begin_read().await?;
        let result = crate::project_capture_marks::acknowledged_capture(&mut read, through).await?;
        read.close().await?;
        Ok(result)
    }

    pub async fn snapshot(&self) -> Result<ProjectSnapshot, ProjectStorageError> {
        let mut transaction = self.pool.begin_read().await?;
        let high_water = crate::project_epoch_reads::checkpoint(&mut transaction).await?;
        Ok(ProjectSnapshot {
            transaction,
            high_water,
            project_id: self.project_id.clone(),
        })
    }

    pub async fn has_thread(
        &self,
        thread: &message_board::ThreadId,
    ) -> Result<bool, ProjectStorageError> {
        let mut read = self.pool.begin_read().await?;
        let thread = thread.as_str();
        let present = sqlx_turso::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM threads WHERE thread_id = ?) AS "present!: bool""#,
            thread
        )
        .fetch_one(&mut read)
        .await?;
        read.close().await?;
        Ok(present)
    }

    pub async fn has_post(
        &self,
        post: &message_board::PostId,
    ) -> Result<bool, ProjectStorageError> {
        let mut read = self.pool.begin_read().await?;
        let post = post.as_str();
        let present = sqlx_turso::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM posts WHERE post_id = ?) AS "present!: bool""#,
            post
        )
        .fetch_one(&mut read)
        .await?;
        read.close().await?;
        Ok(present)
    }

    #[tracing::instrument(skip_all, fields(project = %self.project_id.as_str()))]
    pub async fn has_subscription_memberships(
        &self,
        reader: Option<&message_board::Identity>,
    ) -> Result<bool, ProjectStorageError> {
        let mut read = self.pool.begin_read().await?;
        let reader = reader
            .map(|reader| {
                serde_json::to_string(&message_board::RecordedActor::declared(reader.clone()))
            })
            .transpose()?;
        let reader = reader.as_deref();
        let present = sqlx_turso::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM subscription_memberships m JOIN actors a ON a.actor_id=m.reader_actor WHERE ? IS NULL OR a.canonical_identity=?) AS "present!: bool""#, reader, reader)
            .fetch_one(&mut read).await?;
        read.close().await?;
        Ok(present)
    }

    #[tracing::instrument(skip_all, fields(project = %self.project_id.as_str()))]
    pub async fn has_active_inbox_watch(
        &self,
        reader: &message_board::Identity,
    ) -> Result<bool, ProjectStorageError> {
        let mut read = self.pool.begin_read().await?;
        let identity =
            serde_json::to_string(&message_board::RecordedActor::declared(reader.clone()))?;
        let identity = identity.as_str();
        let row = sqlx_turso::query!(r#"WITH reader AS (SELECT actor_id FROM actors WHERE canonical_identity=?)
SELECT EXISTS(SELECT 1 FROM thread_watches WHERE reader_actor=(SELECT actor_id FROM reader) AND active=1)
 OR EXISTS(SELECT 1 FROM project_watches WHERE reader_actor=(SELECT actor_id FROM reader) AND active=1) AS "active!: bool",
 (SELECT active FROM thread_watches WHERE reader_actor=(SELECT actor_id FROM reader) AND active NOT IN (0,1) LIMIT 1) AS "thread_invalid: i64",
 (SELECT active FROM project_watches WHERE reader_actor=(SELECT actor_id FROM reader) AND active NOT IN (0,1) LIMIT 1) AS "project_invalid: i64""#, identity)
            .fetch_one(&mut read).await?;
        read.close().await?;
        match (row.thread_invalid, row.project_invalid) {
            (None, None) => Ok(row.active),
            _ => Err(ProjectStorageError::InvalidFeed),
        }
    }
}
