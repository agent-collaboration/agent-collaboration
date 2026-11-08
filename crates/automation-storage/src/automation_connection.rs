//! Database connection lifecycle; schema setup precedes every repository operation.
use sqlite_storage_maintenance::{
    CommitPoint, CommitSignal, SqliteStorageMaintenanceError, configure_new_database_options,
    incremental_vacuum_step, prepare_existing_database,
};
use sqlx::{
    Connection, Sqlite, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteSynchronous},
};
use std::{
    ops::{Deref, DerefMut},
    path::Path,
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("wake request refused: {0:?}")]
    WakeRefused(collaboration_protocol::WakeRefusal),
    #[error("automation storage unavailable")]
    Database(#[from] sqlx::Error),
    #[error("automation database maintenance unavailable")]
    SqliteMaintenance(#[from] SqliteStorageMaintenanceError),
    #[error("automation database has unsupported schema version {0}")]
    UnsupportedVersion(i64),
    #[error("automation database contains an unexpected schema; existing data was not replaced")]
    InvalidSchema,
    #[error(
        "legacy message rows remain in {tables:?}; stop the Router and run agent-router-update board-import before starting it"
    )]
    MessageUpgradeRequired { tables: Vec<&'static str> },
    #[error(
        "mail_items contains {rows} rows of pre-release state; clear this development-only state by hand before running agent-router-update board-import or starting the Router"
    )]
    PreReleaseMailPresent { rows: u64 },
    #[error("operation identity already belongs to a different request")]
    OperationConflict,
    #[error(
        "operation was not found; absence of a receipt does not establish native non-execution"
    )]
    OperationNotFound,
    #[error("automation record is invalid or inconsistent")]
    InvalidRecord,
    #[error("event cursor must reference a retained sequence with a consistent observation time")]
    InvalidEventCursor,
    #[error(
        "attempt cursor must preserve its observation time, latest attempt and last emitted identity"
    )]
    InvalidAttemptCursor,
    #[error("push identity already belongs to different content")]
    PushContentConflict,
    // Workflow operations translate these failures from their message-store boundary.
    #[error("push record id already exists")]
    PushAlreadyExists,
    #[error("push record was not found in message storage")]
    PushNotFound,
    #[error("push record delivery state does not permit this transition")]
    PushStateConflict,
    #[error("push record is not permitted for this caller")]
    PushNotPermitted,
    #[error("workflow Run was not found in this automation database")]
    RunNotFound,
    #[error("delivery was not found in this automation database")]
    DeliveryNotFound,
    #[error("instruction was not found in this automation database")]
    InstructionNotFound,
    #[error("instruction changed; inspect the current revision before editing")]
    RevisionConflict {
        current_revision_id: agent_automation::RevisionId,
    },
    #[error("schedule was not found in this automation database")]
    ScheduleNotFound,
    #[error("schedule UUID already exists; pass --overwrite for an intentional replacement")]
    ScheduleImportExists,
    #[error(
        "imported instruction differs from existing shared instruction {instruction_id:?}; explicitly edit it or use a new instruction identity"
    )]
    InstructionImportConflict {
        instruction_id: agent_automation::InstructionId,
        schedule_ids: Vec<agent_automation::ScheduleId>,
    },
    #[error(transparent)]
    InvalidPackage(#[from] agent_automation::PackageError),
    #[error("schedule changed; inspect the latest change identity before editing")]
    ScheduleChangeConflict,
    #[error(
        "native endpoint or execution capabilities unavailable; inspect endpoint before enabling this schedule"
    )]
    ActivationUnavailable,
    #[error("native thread is already owned by schedule {schedule_id:?}")]
    ThreadOwnershipConflict {
        schedule_id: agent_automation::ScheduleId,
    },
    #[error("multiple conflicting Runs prevent safe admission")]
    RunInvariantConflict {
        run_ids: Vec<agent_automation::RunId>,
    },
    #[error("the previous Run has no required continuity summary; inspect summary recovery")]
    MissingContinuity,
    #[error("invalid schedule {field}: {reason}")]
    InvalidSchedule {
        field: &'static str,
        reason: &'static str,
    },
    #[error(transparent)]
    InvalidTiming(#[from] agent_automation::TimingError),
    #[error("wake-up was not found in this automation database")]
    WakeNotFound,
    #[error(
        "wake-up lifecycle does not permit this action in state {state:?}; inspect or create another reminder"
    )]
    WakeLifecycleConflict { state: agent_automation::WakeState },
    #[error("invalid wake-up {field}: {reason}")]
    InvalidWake {
        field: &'static str,
        reason: &'static str,
    },
}

pub struct AutomationStore {
    pub(crate) connection: SqliteConnection,
    commits: CommitSignal,
}

/// How the store's one transaction function opens its transaction.
#[derive(Clone, Copy, Debug)]
pub(crate) enum TransactionStart {
    /// `BEGIN IMMEDIATE`: takes the write lock first. Every mutating method uses it.
    Immediate,
    /// `BEGIN`: a consistent read snapshot.
    Deferred,
}

/// One transaction on the automation database. Committing it is the only way a write
/// becomes visible, and it signals subscribers only when rows changed.
pub(crate) struct AutomationTransaction<'store> {
    transaction: sqlx::Transaction<'store, Sqlite>,
    changes_at_begin: i64,
    commits: &'store CommitSignal,
}

impl AutomationTransaction<'_> {
    /// Commits, then bumps the commit signal if this transaction changed rows.
    pub(crate) async fn commit(mut self) -> Result<(), sqlx::Error> {
        let changed_rows = total_changes(&mut self.transaction).await? != self.changes_at_begin;
        let pending = self.commits.begin_commit(changed_rows);
        pending.checkpoint(CommitPoint::BeforeCommitInvoked).await;
        let committed = self.transaction.commit().await;
        if committed.is_ok() {
            pending.checkpoint(CommitPoint::AfterCommitReturned).await;
        }
        pending.settle(committed.is_ok());
        committed
    }

    /// A known rollback; it never signals.
    pub(crate) async fn rollback(self) -> Result<(), sqlx::Error> {
        self.transaction.rollback().await
    }
}

impl Deref for AutomationTransaction<'_> {
    type Target = SqliteConnection;

    fn deref(&self) -> &SqliteConnection {
        &self.transaction
    }
}

impl DerefMut for AutomationTransaction<'_> {
    fn deref_mut(&mut self) -> &mut SqliteConnection {
        &mut self.transaction
    }
}

/// Runs one write statement as a one-statement transaction through
/// [`AutomationStore::transaction`], so it signals like every other write.
pub(crate) trait OneStatementWrite {
    async fn execute_as_transaction(
        self,
        store: &mut AutomationStore,
    ) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error>;
}

impl<'q, TArguments> OneStatementWrite for sqlx::query::Query<'q, Sqlite, TArguments>
where
    TArguments: sqlx::IntoArguments<Sqlite> + 'q,
{
    async fn execute_as_transaction(
        self,
        store: &mut AutomationStore,
    ) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
        let mut transaction = store.transaction(TransactionStart::Immediate).await?;
        let result = self.execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(result)
    }
}

async fn total_changes(connection: &mut SqliteConnection) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT total_changes()")
        .fetch_one(connection)
        .await
}
impl AutomationStore {
    pub async fn open(path: &Path) -> Result<Self, StorageError> {
        let mut connection = connect(path, true).await?;
        crate::schema_initialization::initialize(&mut connection).await?;
        // Refused legacy upgrades must leave the source file byte-for-byte intact.
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&mut connection)
            .await?;
        prepare_existing_database(&mut connection, path).await?;
        Ok(Self {
            connection,
            commits: CommitSignal::new(),
        })
    }

    /// The one transaction function: every read snapshot and every write goes through it.
    pub(crate) async fn transaction(
        &mut self,
        start: TransactionStart,
    ) -> Result<AutomationTransaction<'_>, sqlx::Error> {
        let mut transaction = match start {
            TransactionStart::Immediate => self.connection.begin_with("BEGIN IMMEDIATE").await?,
            TransactionStart::Deferred => self.connection.begin().await?,
        };
        let changes_at_begin = total_changes(&mut transaction).await?;
        Ok(AutomationTransaction {
            transaction,
            changes_at_begin,
            commits: &self.commits,
        })
    }

    /// A receiver that wakes after each committed transaction that changed rows.
    #[must_use]
    pub fn subscribe_commits(&self) -> tokio::sync::watch::Receiver<u64> {
        self.commits.subscribe()
    }

    /// The store's commit signal, for a test that holds a commit at a [`CommitPoint`].
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn commit_signal(&self) -> &CommitSignal {
        &self.commits
    }

    /// Reclaims one bounded batch of free pages after an automation prune.
    pub async fn incremental_vacuum_step(&mut self) -> Result<(), StorageError> {
        incremental_vacuum_step(&mut self.connection).await?;
        Ok(())
    }

    pub async fn close(self) -> Result<(), StorageError> {
        self.connection.close().await?;
        Ok(())
    }
}

/// Applies the message drop after the upgrade has committed and verified its copy.
///
/// The caller must stop all file owners and verify counts and per-row digests
/// across the source and destination first. Pre-release mail state always refuses.
pub async fn apply_message_drop_migration(path: &Path) -> Result<(), StorageError> {
    let mut connection = connect(path, false).await?;
    let initialized =
        crate::schema_initialization::initialize_after_verified_message_copy(&mut connection).await;
    let closed = connection.close().await.map_err(StorageError::from);
    initialized.and(closed)
}

async fn connect(path: &Path, create_if_missing: bool) -> Result<SqliteConnection, StorageError> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(create_if_missing)
        .foreign_keys(true)
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(Duration::from_secs(1));
    let options = configure_new_database_options(options, path)?;
    Ok(SqliteConnection::connect_with(&options).await?)
}

#[cfg(test)]
#[path = "automation_transaction_test.rs"]
mod transaction_tests;

#[cfg(test)]
mod tests {
    use super::{AutomationStore, SqliteConnection};
    use sqlx::{
        Connection,
        sqlite::{SqliteConnectOptions as ConnectOptions, SqliteJournalMode},
    };

    #[tokio::test]
    async fn fresh_automation_database_opens_in_wal_with_incremental_auto_vacuum() {
        let directory = tempfile::tempdir().expect("automation fixture directory");
        let path = directory.path().join("automation.sqlite");
        let mut store = AutomationStore::open(&path)
            .await
            .expect("fresh automation store should open");

        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut store.connection)
            .await
            .expect("journal mode should be readable");
        let auto_vacuum: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
            .fetch_one(&mut store.connection)
            .await
            .expect("auto-vacuum mode should be readable");

        assert_eq!(journal_mode, "wal");
        assert_eq!(auto_vacuum, 2);
        store.close().await.expect("automation store should close");
    }

    #[tokio::test]
    async fn existing_delete_mode_automation_database_reopens_in_wal_and_preserves_rows() {
        let directory = tempfile::tempdir().expect("automation fixture directory");
        let path = directory.path().join("automation.sqlite");
        let mut seeded_store = AutomationStore::open(&path)
            .await
            .expect("automation schema should initialize");
        sqlx::query(
            "INSERT INTO automation_events(event_id,subject_kind,subject_id,event_kind,event_body_json,recorded_at_ms) VALUES ('retained-event','fixture','owner','fixture','{}',17)",
        )
        .execute(&mut seeded_store.connection)
        .await
        .expect("automation data should be written before switching the fixture");
        seeded_store
            .close()
            .await
            .expect("seeded automation store should close");

        let mut legacy_connection = SqliteConnection::connect_with(
            &ConnectOptions::new()
                .filename(&path)
                .journal_mode(SqliteJournalMode::Delete),
        )
        .await
        .expect("legacy DELETE database should open");
        let legacy_journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut legacy_connection)
            .await
            .expect("legacy journal mode should be readable");
        legacy_connection
            .close()
            .await
            .expect("legacy connection should close");
        assert_eq!(legacy_journal_mode, "delete");

        let mut store = AutomationStore::open(&path)
            .await
            .expect("legacy automation store should reopen");
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut store.connection)
            .await
            .expect("converted journal mode should be readable");
        let auto_vacuum: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
            .fetch_one(&mut store.connection)
            .await
            .expect("converted auto-vacuum mode should be readable");
        let retained_event: Option<String> = sqlx::query_scalar(
            "SELECT event_id FROM automation_events WHERE event_id='retained-event'",
        )
        .fetch_optional(&mut store.connection)
        .await
        .expect("legacy automation event should remain readable");

        assert_eq!(journal_mode, "wal");
        assert_eq!(auto_vacuum, 2);
        assert_eq!(retained_event.as_deref(), Some("retained-event"));
        store.close().await.expect("automation store should close");
    }
}
