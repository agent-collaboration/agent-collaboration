//! The ledger of one-time update steps, kept inside the Fleet database file itself so
//! it always travels with the data it describes, and the catalog of those steps.
//!
//! The update tool writes a step's row when it runs it. A brand-new Fleet store
//! records every catalog step done, since a root with nothing old needs none of them;
//! so an empty or partial ledger always means an update was interrupted.
use crate::{
    LegacyFleetSource, RouterLocalStorageError,
    fleet_cache_schema::CACHE_MIGRATOR,
    legacy_fleet_sources::{carry_legacy_rows, read_legacy_rows, verify_legacy_rows_carried},
    router_local_database::RouterLocalDatabase,
};
use sqlx::{ConnectOptions, Connection, Row};
use sqlx_turso::{Turso, TursoConnectOptions};
use std::path::Path;

/// One one-time step, in the order the update runs them. The update tool runs exactly
/// these, and the Host refuses to start until each is done.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouterUpdateStep {
    /// 0001: the Fleet database gains the update ledger before anything moves.
    CreateLedger,
    /// 0005: the Fleet outbox and the board cursor key move into the Fleet database.
    MergeFleetStores,
    /// 0007: `providers.json` and `ports.json` become `settings.jsonc`, and
    /// `routers.jsonc` is renamed to `agent-sessions.jsonc`.
    CarrySettingsFiles,
    /// 0008: the provider proxy store `state.sqlite` and its lock folder are renamed to
    /// `provider-proxy.sqlite` after a full checkpoint.
    RenameProviderProxyStore,
    /// 0010: `fleet-cache.sqlite` is renamed to `fleet.sqlite`.
    RenameFleetDatabase,
}

impl RouterUpdateStep {
    pub const KNOWN: [Self; 5] = [
        Self::CreateLedger,
        Self::MergeFleetStores,
        Self::CarrySettingsFiles,
        Self::RenameProviderProxyStore,
        Self::RenameFleetDatabase,
    ];

    #[must_use]
    pub const fn number(self) -> UpdateStepNumber {
        match self {
            Self::CreateLedger => UpdateStepNumber::new(1),
            Self::MergeFleetStores => UpdateStepNumber::new(5),
            Self::CarrySettingsFiles => UpdateStepNumber::new(7),
            Self::RenameProviderProxyStore => UpdateStepNumber::new(8),
            Self::RenameFleetDatabase => UpdateStepNumber::new(10),
        }
    }

    /// The step's ledger number as the owner sees it, such as "0001".
    #[must_use]
    pub fn label(self) -> String {
        format!("{:04}", self.number().get())
    }
}

/// A one-time update step's number, such as 1 for step 0001.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UpdateStepNumber(u16);

impl UpdateStepNumber {
    /// Step numbers are literals in the step catalog; a stored 0 is refused when decoded.
    pub const fn new(number: u16) -> Self {
        Self(number)
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// What a ledger row says about its step, or about one source file or table of it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateLedgerEntry {
    Step {
        step: UpdateStepNumber,
        progress: UpdateStepProgress,
    },
    Source {
        step: UpdateStepNumber,
        source: String,
        progress: UpdateSourceProgress,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateStepProgress {
    InProgress,
    Done,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateSourceProgress {
    /// Copied and committed at the destination; the source is still present.
    Written,
    Removed,
}

impl UpdateStepProgress {
    const fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Done => "done",
        }
    }
}

impl UpdateSourceProgress {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Written => "written",
            Self::Removed => "removed",
        }
    }
}

/// The ledger as a read-only inspection found it, without opening the store for writing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoredUpdateLedger {
    /// The Fleet database file isn't there.
    MissingDatabase,
    /// The file exists but was never migrated to carry the ledger table.
    WithoutLedger,
    Present(Vec<UpdateLedgerEntry>),
}

impl StoredUpdateLedger {
    /// The progress the ledger records for `source` within `step`, if any.
    #[must_use]
    pub fn source_progress(
        &self,
        step: UpdateStepNumber,
        source: &str,
    ) -> Option<UpdateSourceProgress> {
        let Self::Present(entries) = self else {
            return None;
        };
        entries.iter().find_map(|entry| match entry {
            UpdateLedgerEntry::Source {
                step: entry_step,
                source: name,
                progress,
            } if *entry_step == step && name == source => Some(*progress),
            _ => None,
        })
    }

    #[must_use]
    pub fn step_done(&self, step: UpdateStepNumber) -> bool {
        match self {
            Self::MissingDatabase | Self::WithoutLedger => false,
            Self::Present(entries) => entries.iter().any(|entry| {
                *entry
                    == UpdateLedgerEntry::Step {
                        step,
                        progress: UpdateStepProgress::Done,
                    }
            }),
        }
    }
}

/// The ledger opened for writing by the update tool, the store's only writer while it runs.
pub struct UpdateLedger {
    database: RouterLocalDatabase,
}

impl UpdateLedger {
    /// Migrates an existing Fleet database so it carries the ledger; never creates the file.
    pub async fn migrate_existing(path: &Path) -> Result<Self, RouterLocalStorageError> {
        Ok(Self {
            database: RouterLocalDatabase::open_existing(path, &CACHE_MIGRATOR).await?,
        })
    }

    /// Creates a missing Fleet database with its ledger in one go.
    pub async fn create(path: &Path) -> Result<Self, RouterLocalStorageError> {
        Ok(Self {
            database: RouterLocalDatabase::open(path, &CACHE_MIGRATOR).await?,
        })
    }

    pub async fn record_step(
        &self,
        step: UpdateStepNumber,
        progress: UpdateStepProgress,
    ) -> Result<(), RouterLocalStorageError> {
        record_steps(&self.database, &[step], progress).await
    }

    pub async fn entries(&self) -> Result<Vec<UpdateLedgerEntry>, RouterLocalStorageError> {
        read_entries(&self.database.pool).await
    }

    /// Step 0005, one source: carries every row of the legacy store into this Fleet
    /// database and records the source `written` in the same transaction.
    pub async fn carry_legacy_source(
        &self,
        source: LegacyFleetSource,
        path: &Path,
    ) -> Result<(), RouterLocalStorageError> {
        let rows = read_legacy_rows(source, path).await?;
        let _writer = self.database.writer.lock().await;
        let mut connection = self.database.pool.acquire().await?;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let carried = async {
            carry_legacy_rows(&mut transaction, &rows).await?;
            write_ledger_row(
                &mut transaction,
                RouterUpdateStep::MergeFleetStores.number(),
                source.file_name(),
                UpdateSourceProgress::Written.as_str(),
            )
            .await
        }
        .await;
        match carried {
            Ok(()) => transaction.commit().await?,
            Err(error) => {
                transaction.rollback().await?;
                return Err(error);
            }
        }
        Ok(())
    }

    /// Step 0005, before a source is removed: every one of its rows is here, unchanged.
    pub async fn verify_legacy_source_carried(
        &self,
        source: LegacyFleetSource,
        path: &Path,
    ) -> Result<(), RouterLocalStorageError> {
        let rows = read_legacy_rows(source, path).await?;
        verify_legacy_rows_carried(&self.database, &rows).await
    }

    /// Records one file's progress within `step`, such as `settings.jsonc` written.
    pub async fn record_step_source(
        &self,
        step: RouterUpdateStep,
        source: &str,
        progress: UpdateSourceProgress,
    ) -> Result<(), RouterLocalStorageError> {
        let _writer = self.database.writer.lock().await;
        let mut connection = self.database.pool.acquire().await?;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        write_ledger_row(&mut transaction, step.number(), source, progress.as_str()).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn record_source(
        &self,
        source: LegacyFleetSource,
        progress: UpdateSourceProgress,
    ) -> Result<(), RouterLocalStorageError> {
        let _writer = self.database.writer.lock().await;
        let mut connection = self.database.pool.acquire().await?;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        write_ledger_row(
            &mut transaction,
            RouterUpdateStep::MergeFleetStores.number(),
            source.file_name(),
            progress.as_str(),
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Step 0010, before the rename: moves every page out of the write-ahead log into
    /// the database file, then closes the one connection.
    pub async fn checkpoint_and_close(self) -> Result<(), RouterLocalStorageError> {
        let checkpointed = sqlx::query::<Turso>("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.database.pool)
            .await;
        self.database.close().await;
        checkpointed?;
        Ok(())
    }

    pub async fn close(self) {
        self.database.close().await;
    }
}

/// A Fleet store this open just created has nothing old in it: every catalog step is done.
pub(crate) async fn record_catalog_done_in_new_store(
    database: &RouterLocalDatabase,
) -> Result<(), RouterLocalStorageError> {
    let catalog = RouterUpdateStep::KNOWN.map(RouterUpdateStep::number);
    record_steps(database, &catalog, UpdateStepProgress::Done).await
}

async fn record_steps(
    database: &RouterLocalDatabase,
    steps: &[UpdateStepNumber],
    progress: UpdateStepProgress,
) -> Result<(), RouterLocalStorageError> {
    let _writer = database.writer.lock().await;
    let mut connection = database.pool.acquire().await?;
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    for step in steps {
        write_ledger_row(&mut transaction, *step, "", progress.as_str()).await?;
    }
    transaction.commit().await?;
    Ok(())
}

/// Reads the ledger through a read-only open, which takes no store lock, so a running Host
/// holding the store isn't disturbed and the file is never created or migrated.
pub async fn read_update_ledger(
    path: &Path,
) -> Result<StoredUpdateLedger, RouterLocalStorageError> {
    if !tokio::fs::try_exists(path).await.map_err(sqlx::Error::Io)? {
        return Ok(StoredUpdateLedger::MissingDatabase);
    }
    let mut connection = TursoConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await?;
    let has_ledger = sqlx::query::<Turso>(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'update_steps'",
    )
    .fetch_optional(&mut connection)
    .await?
    .is_some();
    let ledger = if has_ledger {
        StoredUpdateLedger::Present(read_entries(&mut connection).await?)
    } else {
        StoredUpdateLedger::WithoutLedger
    };
    connection.close().await?;
    Ok(ledger)
}

async fn read_entries<'connection>(
    executor: impl sqlx::Executor<'connection, Database = Turso>,
) -> Result<Vec<UpdateLedgerEntry>, RouterLocalStorageError> {
    let rows =
        sqlx::query::<Turso>("SELECT step, source, state FROM update_steps ORDER BY step, source")
            .fetch_all(executor)
            .await?;
    rows.iter()
        .map(|row| {
            decode_entry(StoredLedgerRow {
                step: row.try_get("step")?,
                source: row.try_get("source")?,
                state: row.try_get("state")?,
            })
        })
        .collect()
}

struct StoredLedgerRow {
    step: i64,
    source: String,
    state: String,
}

fn decode_entry(row: StoredLedgerRow) -> Result<UpdateLedgerEntry, RouterLocalStorageError> {
    let step = u16::try_from(row.step)
        .ok()
        .filter(|number| *number != 0)
        .map(UpdateStepNumber::new)
        .ok_or(RouterLocalStorageError::InvalidRecordKey {
            field: "update_steps.step",
        })?;
    let invalid_state = RouterLocalStorageError::InvalidRecordKey {
        field: "update_steps.state",
    };
    if row.source.is_empty() {
        let progress = match row.state.as_str() {
            "in_progress" => UpdateStepProgress::InProgress,
            "done" => UpdateStepProgress::Done,
            _ => return Err(invalid_state),
        };
        return Ok(UpdateLedgerEntry::Step { step, progress });
    }
    let progress = match row.state.as_str() {
        "written" => UpdateSourceProgress::Written,
        "removed" => UpdateSourceProgress::Removed,
        _ => return Err(invalid_state),
    };
    Ok(UpdateLedgerEntry::Source {
        step,
        source: row.source,
        progress,
    })
}

#[cfg(test)]
#[path = "update_ledger_test.rs"]
mod tests;

/// Writes one ledger row (a step's when `source` is empty) inside the caller's transaction.
async fn write_ledger_row(
    transaction: &mut sqlx::Transaction<'_, Turso>,
    step: UpdateStepNumber,
    source: &str,
    state: &str,
) -> Result<(), RouterLocalStorageError> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    sqlx::query::<Turso>(
        "INSERT INTO update_steps(id, step, source, state, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT(step, source) DO UPDATE SET state = excluded.state, updated_at = excluded.updated_at",
    )
    .bind(uuid::Uuid::now_v7().to_string())
    .bind(i64::from(step.get()))
    .bind(source)
    .bind(state)
    .bind(&now)
    .bind(&now)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
