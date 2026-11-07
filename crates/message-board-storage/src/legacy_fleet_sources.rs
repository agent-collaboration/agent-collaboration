//! The two older stores update step 0005 merges into the Fleet database, and how
//! their rows are read, carried and checked. Nothing else opens these files.
//!
//! A legacy store is first brought to its last old shape by its own old migrations,
//! exactly as the old Host would have on its next start, so an older store's rows move
//! with the values that Host would have given them.
use crate::{
    RouterLocalStorageError,
    cursor_store::{CursorSigningKey, CursorSigningKeyId},
    fleet_outbox::{FleetOutboxEntry, decode_entry, decode_input},
    router_local_database::RouterLocalDatabase,
};
use sqlx::{Row, migrate::Migrator};
use sqlx_turso::Turso;
use std::{collections::BTreeMap, path::Path};

pub(crate) static LEGACY_OUTBOX_MIGRATOR: Migrator = sqlx::migrate!("./fleet_outbox_migrations");
static LEGACY_CURSOR_MIGRATOR: Migrator = sqlx::migrate!("./cursor_migrations");

/// One older store whose rows step 0005 moves into the Fleet database.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegacyFleetSource {
    /// `fleet-outbox.sqlite`: Fleet requests and project creation inputs.
    Outbox,
    /// `board-cursors.sqlite`: the board cursor signing key.
    CursorKey,
}

impl LegacyFleetSource {
    pub const ALL: [Self; 2] = [Self::Outbox, Self::CursorKey];

    /// The store's file name in the Router's collaboration directory, and its ledger source.
    #[must_use]
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Outbox => "fleet-outbox.sqlite",
            Self::CursorKey => "board-cursors.sqlite",
        }
    }

    const fn migrator(self) -> &'static Migrator {
        match self {
            Self::Outbox => &LEGACY_OUTBOX_MIGRATOR,
            Self::CursorKey => &LEGACY_CURSOR_MIGRATOR,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OutboxRequestRow {
    request_id: String,
    canonical_hash: String,
    request: String,
    state: String,
    settled_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CreationInputRow {
    request_id: String,
    canonical_hash: String,
    input: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CursorKeyRow {
    key_id: String,
    key_bytes: Vec<u8>,
}

/// Every row of one legacy store, in key order.
pub(crate) enum LegacyRows {
    Outbox {
        requests: Vec<OutboxRequestRow>,
        creation_inputs: Vec<CreationInputRow>,
    },
    CursorKey {
        keys: Vec<CursorKeyRow>,
    },
}

/// Reads every row of an existing legacy store and checks each through the decoder that
/// owns its value; a missing or malformed file, or any row that decoder refuses, is an
/// error before a caller can copy or remove anything.
pub(crate) async fn read_legacy_rows(
    source: LegacyFleetSource,
    path: &Path,
) -> Result<LegacyRows, RouterLocalStorageError> {
    let database = RouterLocalDatabase::open_existing(path, source.migrator()).await?;
    let rows = match source {
        LegacyFleetSource::Outbox => read_outbox(&database).await,
        LegacyFleetSource::CursorKey => read_cursor_keys(&database).await,
    };
    database.close().await;
    let rows = rows?;
    validate_legacy_rows(source, &rows)?;
    Ok(rows)
}

/// Reads a legacy store and checks every row, changing nothing, so step 0005 can refuse
/// before it carries or removes any store.
pub async fn check_legacy_source(
    source: LegacyFleetSource,
    path: &Path,
) -> Result<(), RouterLocalStorageError> {
    read_legacy_rows(source, path).await.map(drop)
}

/// Each value must be one the current Fleet code would read back: outbox requests through
/// the outbox decoder, creation inputs against their own request in the same store, and
/// the cursor key as the single key the cursor store accepts.
fn validate_legacy_rows(
    source: LegacyFleetSource,
    rows: &LegacyRows,
) -> Result<(), RouterLocalStorageError> {
    match rows {
        LegacyRows::Outbox {
            requests,
            creation_inputs,
        } => {
            let mut decoded: BTreeMap<&str, FleetOutboxEntry> = BTreeMap::new();
            for row in requests {
                let entry = decode_entry(
                    &row.request_id,
                    &row.canonical_hash,
                    &row.request,
                    &row.state,
                    row.settled_at_ms,
                )
                .map_err(|reason| {
                    invalid_row(source, "fleet_outbox_requests", &row.request_id, reason)
                })?;
                decoded.insert(&row.request_id, entry);
            }
            for row in creation_inputs {
                let invalid = |reason| {
                    invalid_row(source, "project_creation_inputs", &row.request_id, reason)
                };
                let Some(request) = decoded.get(row.request_id.as_str()) else {
                    return Err(invalid(RouterLocalStorageError::InvalidRecord));
                };
                decode_input(&row.input, &row.canonical_hash, request).map_err(invalid)?;
            }
        }
        LegacyRows::CursorKey { keys } => {
            for (position, row) in keys.iter().enumerate() {
                let invalid =
                    |reason| invalid_row(source, "cursor_signing_key", &row.key_id, reason);
                if position > 0 {
                    // The cursor store holds exactly one key and refuses a second.
                    return Err(invalid(RouterLocalStorageError::InvalidRecord));
                }
                CursorSigningKeyId::try_from(row.key_id.clone()).map_err(invalid)?;
                CursorSigningKey::try_from(row.key_bytes.clone()).map_err(invalid)?;
            }
        }
    }
    Ok(())
}

fn invalid_row(
    source: LegacyFleetSource,
    table: &'static str,
    key: &str,
    reason: RouterLocalStorageError,
) -> RouterLocalStorageError {
    RouterLocalStorageError::InvalidLegacyRow {
        store: source.file_name(),
        table,
        key: key.to_owned(),
        reason: Box::new(reason),
    }
}

async fn read_outbox(
    database: &RouterLocalDatabase,
) -> Result<LegacyRows, RouterLocalStorageError> {
    let requests = sqlx::query::<Turso>(
        "SELECT request_id, canonical_hash, request, state, settled_at_ms FROM fleet_outbox_requests ORDER BY request_id",
    )
    .fetch_all(&database.pool)
    .await?
    .iter()
    .map(|row| {
        Ok(OutboxRequestRow {
            request_id: row.try_get("request_id")?,
            canonical_hash: row.try_get("canonical_hash")?,
            request: row.try_get("request")?,
            state: row.try_get("state")?,
            settled_at_ms: row.try_get("settled_at_ms")?,
        })
    })
    .collect::<Result<Vec<_>, sqlx::Error>>()?;
    let creation_inputs = sqlx::query::<Turso>(
        "SELECT request_id, canonical_hash, input FROM project_creation_inputs ORDER BY request_id",
    )
    .fetch_all(&database.pool)
    .await?
    .iter()
    .map(|row| {
        Ok(CreationInputRow {
            request_id: row.try_get("request_id")?,
            canonical_hash: row.try_get("canonical_hash")?,
            input: row.try_get("input")?,
        })
    })
    .collect::<Result<Vec<_>, sqlx::Error>>()?;
    Ok(LegacyRows::Outbox {
        requests,
        creation_inputs,
    })
}

async fn read_cursor_keys(
    database: &RouterLocalDatabase,
) -> Result<LegacyRows, RouterLocalStorageError> {
    let keys =
        sqlx::query::<Turso>("SELECT key_id, key_bytes FROM cursor_signing_key ORDER BY key_id")
            .fetch_all(&database.pool)
            .await?
            .iter()
            .map(|row| {
                let key_id: Option<String> = row.try_get("key_id")?;
                let Some(key_id) = key_id else {
                    // SQLite lets a TEXT primary key hold NULL; the cursor store never does.
                    return Err(invalid_row(
                        LegacyFleetSource::CursorKey,
                        "cursor_signing_key",
                        "NULL",
                        RouterLocalStorageError::InvalidRecordKey {
                            field: "cursor_signing_key.key_id",
                        },
                    ));
                };
                Ok(CursorKeyRow {
                    key_id,
                    key_bytes: row.try_get("key_bytes")?,
                })
            })
            .collect::<Result<Vec<_>, RouterLocalStorageError>>()?;
    Ok(LegacyRows::CursorKey { keys })
}

/// Inserts the rows into the Fleet database inside the caller's transaction. A row
/// already there with the same values was carried by an earlier run; one with
/// different values is a conflict, never overwritten.
pub(crate) async fn carry_legacy_rows(
    transaction: &mut sqlx::Transaction<'_, Turso>,
    rows: &LegacyRows,
) -> Result<(), RouterLocalStorageError> {
    match rows {
        LegacyRows::Outbox {
            requests,
            creation_inputs,
        } => {
            for row in requests {
                match fleet_outbox_request(transaction, &row.request_id).await? {
                    Some(existing) if existing == *row => {}
                    Some(_) => return Err(conflict("fleet_outbox_requests")),
                    None => {
                        sqlx::query::<Turso>("INSERT INTO fleet_outbox_requests(request_id, canonical_hash, request, state, settled_at_ms) VALUES (?, ?, ?, ?, ?)")
                            .bind(&row.request_id)
                            .bind(&row.canonical_hash)
                            .bind(&row.request)
                            .bind(&row.state)
                            .bind(row.settled_at_ms)
                            .execute(&mut **transaction)
                            .await?;
                    }
                }
            }
            for row in creation_inputs {
                match fleet_creation_input(transaction, &row.request_id).await? {
                    Some(existing) if existing == *row => {}
                    Some(_) => return Err(conflict("fleet_project_creation_inputs")),
                    None => {
                        sqlx::query::<Turso>("INSERT INTO fleet_project_creation_inputs(request_id, canonical_hash, input) VALUES (?, ?, ?)")
                            .bind(&row.request_id)
                            .bind(&row.canonical_hash)
                            .bind(&row.input)
                            .execute(&mut **transaction)
                            .await?;
                    }
                }
            }
        }
        LegacyRows::CursorKey { keys } => {
            for row in keys {
                match fleet_cursor_key(transaction, &row.key_id).await? {
                    Some(existing) if existing == *row => {}
                    Some(_) => return Err(conflict("fleet_cursor_signing_key")),
                    None => {
                        sqlx::query::<Turso>(
                            "INSERT INTO fleet_cursor_signing_key(key_id, key_bytes) VALUES (?, ?)",
                        )
                        .bind(&row.key_id)
                        .bind(&row.key_bytes)
                        .execute(&mut **transaction)
                        .await?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Checks that every legacy row is in the Fleet database with exactly its values.
pub(crate) async fn verify_legacy_rows_carried(
    database: &RouterLocalDatabase,
    rows: &LegacyRows,
) -> Result<(), RouterLocalStorageError> {
    let mut connection = database.pool.acquire().await?;
    match rows {
        LegacyRows::Outbox {
            requests,
            creation_inputs,
        } => {
            for row in requests {
                if fleet_outbox_request(&mut connection, &row.request_id)
                    .await?
                    .as_ref()
                    != Some(row)
                {
                    return Err(not_carried("fleet_outbox_requests"));
                }
            }
            for row in creation_inputs {
                if fleet_creation_input(&mut connection, &row.request_id)
                    .await?
                    .as_ref()
                    != Some(row)
                {
                    return Err(not_carried("fleet_project_creation_inputs"));
                }
            }
        }
        LegacyRows::CursorKey { keys } => {
            for row in keys {
                if fleet_cursor_key(&mut connection, &row.key_id)
                    .await?
                    .as_ref()
                    != Some(row)
                {
                    return Err(not_carried("fleet_cursor_signing_key"));
                }
            }
        }
    }
    Ok(())
}

async fn fleet_outbox_request(
    connection: &mut sqlx_turso::TursoConnection,
    request_id: &str,
) -> Result<Option<OutboxRequestRow>, RouterLocalStorageError> {
    let Some(row) = sqlx::query::<Turso>("SELECT request_id, canonical_hash, request, state, settled_at_ms FROM fleet_outbox_requests WHERE request_id = ?")
        .bind(request_id)
        .fetch_optional(connection)
        .await?
    else {
        return Ok(None);
    };
    Ok(Some(OutboxRequestRow {
        request_id: row.try_get("request_id")?,
        canonical_hash: row.try_get("canonical_hash")?,
        request: row.try_get("request")?,
        state: row.try_get("state")?,
        settled_at_ms: row.try_get("settled_at_ms")?,
    }))
}

async fn fleet_creation_input(
    connection: &mut sqlx_turso::TursoConnection,
    request_id: &str,
) -> Result<Option<CreationInputRow>, RouterLocalStorageError> {
    let Some(row) = sqlx::query::<Turso>("SELECT request_id, canonical_hash, input FROM fleet_project_creation_inputs WHERE request_id = ?")
        .bind(request_id)
        .fetch_optional(connection)
        .await?
    else {
        return Ok(None);
    };
    Ok(Some(CreationInputRow {
        request_id: row.try_get("request_id")?,
        canonical_hash: row.try_get("canonical_hash")?,
        input: row.try_get("input")?,
    }))
}

async fn fleet_cursor_key(
    connection: &mut sqlx_turso::TursoConnection,
    key_id: &str,
) -> Result<Option<CursorKeyRow>, RouterLocalStorageError> {
    let Some(row) = sqlx::query::<Turso>(
        "SELECT key_id, key_bytes FROM fleet_cursor_signing_key WHERE key_id = ?",
    )
    .bind(key_id)
    .fetch_optional(connection)
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(CursorKeyRow {
        key_id: row.try_get("key_id")?,
        key_bytes: row.try_get("key_bytes")?,
    }))
}

const fn conflict(table: &'static str) -> RouterLocalStorageError {
    RouterLocalStorageError::LegacyRowConflict { table }
}

const fn not_carried(table: &'static str) -> RouterLocalStorageError {
    RouterLocalStorageError::LegacyRowNotCarried { table }
}
