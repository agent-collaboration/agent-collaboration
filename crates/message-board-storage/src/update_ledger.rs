//! The update tool's ledger of one-time steps, kept inside the Fleet database file
//! itself so it always travels with the data it describes.
use crate::{
    RouterLocalStorageError, fleet_cache::CACHE_MIGRATOR,
    router_local_database::RouterLocalDatabase,
};
use sqlx::{ConnectOptions, Connection, Row};
use sqlx_turso::{Turso, TursoConnectOptions};
use std::path::Path;

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
        let _writer = self.database.writer.lock().await;
        let mut connection = self.database.pool.acquire().await?;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        sqlx::query::<Turso>(
            "INSERT INTO update_steps(id, step, source, state, created_at, updated_at) \
             VALUES (?, ?, '', ?, ?, ?) \
             ON CONFLICT(step, source) DO UPDATE SET state = excluded.state, updated_at = excluded.updated_at",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(i64::from(step.get()))
        .bind(progress.as_str())
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn entries(&self) -> Result<Vec<UpdateLedgerEntry>, RouterLocalStorageError> {
        read_entries(&self.database.pool).await
    }

    pub async fn close(self) {
        self.database.close().await;
    }
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
