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
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&mut *connection)
            .await?;
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

#[cfg(test)]
mod tests {
    use super::RouterLocalDatabase;
    use sqlx::{
        Connection,
        sqlite::{SqliteConnectOptions, SqliteConnection, SqliteJournalMode},
    };

    async fn journal_mode(database: &RouterLocalDatabase) -> String {
        let mut connection = database
            .pool
            .acquire()
            .await
            .expect("Router-local database should be queryable");
        sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut *connection)
            .await
            .expect("journal mode should be readable")
    }

    #[tokio::test]
    async fn fresh_router_local_database_opens_in_wal_mode() {
        let directory = tempfile::tempdir().expect("Router-local database fixture");
        let path = directory.path().join("fresh.sqlite");
        let database = RouterLocalDatabase::open(&path, &sqlx::migrate::Migrator::DEFAULT)
            .await
            .expect("fresh Router-local database should open");

        assert_eq!(journal_mode(&database).await, "wal");
        database.close().await;
    }

    #[tokio::test]
    async fn existing_delete_mode_router_local_database_reopens_in_wal_mode() {
        let directory = tempfile::tempdir().expect("Router-local database fixture");
        let path = directory.path().join("legacy.sqlite");
        let mut legacy = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Delete),
        )
        .await
        .expect("legacy DELETE database should open");
        sqlx::query("CREATE TABLE retained_legacy_row(value TEXT PRIMARY KEY)")
            .execute(&mut legacy)
            .await
            .expect("legacy fixture schema should be created");
        sqlx::query("INSERT INTO retained_legacy_row(value) VALUES ('retained')")
            .execute(&mut legacy)
            .await
            .expect("legacy row should be inserted");
        let legacy_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut legacy)
            .await
            .expect("legacy journal mode should be readable");
        legacy
            .close()
            .await
            .expect("legacy connection should close");
        assert_eq!(legacy_mode, "delete");

        let database = RouterLocalDatabase::open(&path, &sqlx::migrate::Migrator::DEFAULT)
            .await
            .expect("legacy Router-local database should reopen");

        assert_eq!(journal_mode(&database).await, "wal");
        database.close().await;
    }
}
