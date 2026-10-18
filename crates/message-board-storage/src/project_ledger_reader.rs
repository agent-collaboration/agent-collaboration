//! Orphaned files expose only ledger lookups, with no migration or writer capability.
use crate::{ProjectSnapshot, ProjectStorageError};
use message_board::{
    FeedCursor, ProjectId, Record, Refusal, RequestId, RequestLedgerEntry, VoidCause,
};
use sqlx::{Pool, pool::PoolOptions};
use sqlx_turso::{Turso, TursoConnectOptions};
use std::path::Path;

pub struct ProjectLedgerReader {
    pool: Pool<Turso>,
    project: ProjectId,
}
impl ProjectLedgerReader {
    pub async fn open(path: &Path, project: ProjectId) -> Result<Self, ProjectStorageError> {
        tracing::trace!(target: "message_board_storage::open", project = %project.as_str(), "rejected project ledger open");
        let options = TursoConnectOptions::new()
            .filename(path)
            .create_if_missing(false);
        let pool = PoolOptions::<Turso>::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        if let Err(error) = crate::project_store::verify_move_integrity(&pool).await {
            pool.close().await;
            return Err(error);
        }
        let tables = sqlx_turso::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM sqlite_schema WHERE type = 'table' AND name IN ('record_identities', 'project_feed', 'feed_checkpoint', 'request_ledger', 'project_state')"#).fetch_one(&pool).await?;
        if tables != 5 {
            pool.close().await;
            return Err(ProjectStorageError::InvalidSchema);
        }
        Ok(Self { pool, project })
    }
    async fn snapshot(&self) -> Result<ProjectSnapshot, ProjectStorageError> {
        let mut transaction = self.pool.begin().await?;
        let high_water = crate::project_store::checkpoint(&mut transaction).await?;
        Ok(ProjectSnapshot {
            transaction,
            high_water,
            project_id: self.project.clone(),
            _access_guard: None,
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
    pub async fn rejection_reason(&self) -> Result<Option<Refusal>, ProjectStorageError> {
        let mut snapshot = self.snapshot().await?;
        let ledger = snapshot.project_rejection_request().await?;
        snapshot.finish().await?;
        Ok(ledger.and_then(|ledger| match ledger {
            RequestLedgerEntry {
                decision:
                    message_board::RequestDecision::Voided {
                        cause: VoidCause::ProjectRejected { reason },
                    },
                ..
            } => Some(*reason),
            _ => None,
        }))
    }
    pub async fn high_water(&self) -> Result<FeedCursor, ProjectStorageError> {
        let snapshot = self.snapshot().await?;
        let high_water = snapshot.high_water();
        snapshot.finish().await?;
        Ok(high_water)
    }
    pub async fn close(&self) {
        self.pool.close().await;
    }
}
