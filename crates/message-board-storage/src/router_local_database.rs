//! Shared native opening policy for three separate, never-replicated Router files.
use sqlx::{Connection, Pool, migrate::Migrator, pool::PoolOptions};
use sqlx_turso::{Turso, TursoConnectOptions};
use std::path::Path;
use tokio::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum RouterLocalStorageError {
    #[error("Router-local storage is unavailable: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Router-local migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("Router-local stored value is invalid")]
    InvalidRecord,
    #[error("Router-local stored record is invalid")]
    InvalidRecordAt { record: message_board::RecordRef },
    #[error("Router-local stored identity is invalid in {field}")]
    InvalidRecordKey { field: &'static str },
    #[error("legacy cursor state is invalid")]
    InvalidCursor,
    #[error("request identity has different content")]
    ConflictingRequest,
    #[error("fleet cache page leaves a position gap")]
    PositionGap,
    #[error("Router-local data cannot be encoded: {0}")]
    Encoding(#[from] serde_json::Error),
}

pub(crate) struct RouterLocalDatabase {
    pub(crate) pool: Pool<Turso>,
    pub(crate) writer: Mutex<()>,
}

impl RouterLocalDatabase {
    pub(crate) async fn open(
        path: &Path,
        migrator: &Migrator,
    ) -> Result<Self, RouterLocalStorageError> {
        let options = TursoConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        let pool = PoolOptions::<Turso>::new()
            .max_connections(1)
            .min_connections(1)
            .connect_with(options)
            .await?;
        let mut connection = pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *connection)
            .await?;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            migrator.run_direct(None, &mut *transaction, false).await?;
            if !sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut *transaction)
                .await?
                .is_empty()
            {
                return Err(RouterLocalStorageError::InvalidRecord);
            }
            Ok::<(), RouterLocalStorageError>(())
        }
        .await;
        let migration = match result {
            Ok(()) => transaction
                .commit()
                .await
                .map_err(RouterLocalStorageError::Database),
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        };
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *connection)
            .await?;
        migration?;
        drop(connection);
        Ok(Self {
            pool,
            writer: Mutex::new(()),
        })
    }
    pub(crate) async fn close(self) {
        self.pool.close().await;
    }
}
