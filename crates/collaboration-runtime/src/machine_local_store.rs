//! machine_local.sqlite: the Router core's own Mac state (owner, 2026-11-15,
//! record:210: "no providers database is different from our core database").
//! It holds the central link's outbox (and, with 2c, its inbox); provider
//! operations stay in provider-operations.sqlite. Stock sqlx-sqlite with its
//! own migrations; every write goes through one signalled transaction.
use sqlite_storage_maintenance::{
    CommitSignal, SignaledTransaction, TransactionStart, configure_new_database_options,
    prepare_existing_database, sqlite_total_changes,
};
use sqlx::{
    Connection, SqliteConnection,
    migrate::MigrateError,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
};
use std::{path::Path, time::Duration};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./machine_local_migrations");

/// The file's name in the Router's state directory.
pub const MACHINE_LOCAL_DATABASE: &str = "machine_local.sqlite";

#[derive(Debug, thiserror::Error)]
pub enum MachineLocalStoreError {
    #[error("machine-local storage unavailable")]
    Database(#[from] sqlx::Error),
    #[error("machine-local migration failed")]
    Migration(#[from] MigrateError),
    #[error("machine-local storage maintenance failed")]
    StorageMaintenance(#[from] sqlite_storage_maintenance::SqliteStorageMaintenanceError),
}

pub struct MachineLocalStore {
    pub(crate) connection: SqliteConnection,
    commits: CommitSignal,
}

impl MachineLocalStore {
    /// Opens (creating if missing) and migrates the database. It lives in the
    /// Router's private state directory, like provider-operations.sqlite.
    pub async fn open(path: &Path) -> Result<Self, MachineLocalStoreError> {
        let options = configure_new_database_options(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
                .foreign_keys(true)
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Full)
                .busy_timeout(Duration::from_secs(1)),
            path,
        )?;
        let mut connection = SqliteConnection::connect_with(&options).await?;
        prepare_existing_database(&mut connection, path).await?;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        MIGRATOR.run_direct(None, &mut *transaction, false).await?;
        transaction.commit().await?;
        Ok(Self {
            connection,
            commits: CommitSignal::new(),
        })
    }

    /// The one transaction function: it signals [`Self::subscribe_commits`]
    /// only after a commit that changed rows.
    pub(crate) async fn transaction(
        &mut self,
        start: TransactionStart,
    ) -> Result<SignaledTransaction<'_, sqlx::Sqlite>, sqlx::Error> {
        let transaction = self.connection.begin_with(start.statement()).await?;
        SignaledTransaction::counting(transaction, sqlite_total_changes, &self.commits).await
    }

    #[must_use]
    pub fn subscribe_commits(&self) -> tokio::sync::watch::Receiver<u64> {
        self.commits.subscribe()
    }

    pub async fn close(self) -> Result<(), MachineLocalStoreError> {
        self.connection.close().await?;
        Ok(())
    }
}

/// One statement as its own signalled transaction.
pub(crate) trait MachineLocalWrite {
    async fn execute_as_transaction(
        self,
        store: &mut MachineLocalStore,
    ) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error>;
}

impl<'q, TArguments> MachineLocalWrite for sqlx::query::Query<'q, sqlx::Sqlite, TArguments>
where
    TArguments: sqlx::IntoArguments<sqlx::Sqlite> + 'q,
{
    async fn execute_as_transaction(
        self,
        store: &mut MachineLocalStore,
    ) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        let mut transaction = store.transaction(TransactionStart::Immediate).await?;
        let result = self.execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(result)
    }
}

#[cfg(test)]
#[path = "machine_local_store_test.rs"]
mod tests;
