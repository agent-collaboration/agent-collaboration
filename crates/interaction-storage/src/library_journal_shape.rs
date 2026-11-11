//! The durable-steps journal tables an interaction store may hold, in exactly
//! one shape a released library left or the pinned library leaves. Admission
//! runs before the library's own journal migration, so a store written by an
//! older Router is admitted in its released shape; the pinned library then
//! adopts it inside the same owner transaction, and `require_standard` checks
//! the converted shape before anything commits. Any other combination of
//! tables or columns is refused, never repaired.
//!
//! - `MarkerV1`: agent-fleet-sync b9da4b5 (Router 0.1.125): journal 0001 with
//!   its `_fleet_sync_schema` version marker and SQLx's standard receipts.
//! - `ExtendedReceipts`: agent-fleet-sync 0b27429 (Routers 0.1.126 to 0.1.140):
//!   journal metadata (0002), the rebuilt marker and the extended receipts.
//! - `Standard`: the pinned library after adoption (f052831): SQLx's six
//!   receipt columns, journal metadata, and no marker (its 0003 drops it).

use crate::StorageError;
use sqlx_turso::TursoConnection;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum JournalShape {
    /// No journal tables: a store the journal hasn't been created in.
    Absent,
    MarkerV1,
    ExtendedReceipts,
    Standard,
}

impl JournalShape {
    /// Whether the store still holds the released `_fleet_sync_schema` marker.
    pub(super) fn has_marker(self) -> bool {
        matches!(self, Self::MarkerV1 | Self::ExtendedReceipts)
    }
}

const RECEIPTS: &str = "_fleet_sync_migrations";
const OPERATIONS: &str = "_fleet_sync_operations";
const STEPS: &str = "_fleet_sync_steps";
const MARKER: &str = "_fleet_sync_schema";

/// One column as `pragma_table_info` reports it: name, declared type, NOT NULL, primary-key position.
type Column = (&'static str, &'static str, i64, i64);

const STANDARD_RECEIPTS: &[Column] = &[
    ("version", "BIGINT", 0, 1),
    ("description", "TEXT", 1, 0),
    ("installed_on", "TIMESTAMP", 1, 0),
    ("success", "BOOLEAN", 1, 0),
    ("checksum", "BLOB", 1, 0),
    ("execution_time", "BIGINT", 1, 0),
];
const EXTENDED_RECEIPTS: &[Column] = &[
    ("id", "TEXT", 1, 1),
    ("version", "BIGINT", 1, 0),
    ("description", "TEXT", 1, 0),
    ("installed_on", "TIMESTAMP", 1, 0),
    ("success", "BOOLEAN", 1, 0),
    ("checksum", "BLOB", 1, 0),
    ("execution_time", "BIGINT", 1, 0),
    ("created_at", "INTEGER", 1, 0),
    ("updated_at", "INTEGER", 1, 0),
];
const OPERATIONS_V1: &[Column] = &[
    ("operation_id", "TEXT", 1, 1),
    ("kind", "TEXT", 1, 0),
    ("kind_version", "INTEGER", 1, 0),
    ("request_key", "TEXT", 1, 0),
    ("input", "BLOB", 0, 0),
    ("input_digest", "BLOB", 1, 0),
    ("state", "TEXT", 1, 0),
    ("result", "BLOB", 0, 0),
    ("failure", "TEXT", 0, 0),
    ("finished_at", "INTEGER", 0, 0),
    ("next_attempt_at", "INTEGER", 0, 0),
    ("attempt", "INTEGER", 1, 0),
    ("drive_token", "TEXT", 0, 0),
    ("drive_until", "INTEGER", 0, 0),
];
const OPERATION_METADATA: &[Column] = &[
    ("created_at", "INTEGER", 1, 0),
    ("updated_at", "INTEGER", 1, 0),
];
const STEPS_V1: &[Column] = &[
    ("operation_id", "TEXT", 1, 1),
    ("position", "INTEGER", 1, 2),
    ("step_name", "TEXT", 1, 0),
    ("kind", "TEXT", 1, 0),
    ("intent", "TEXT", 0, 0),
    ("output", "BLOB", 0, 0),
    ("first_attempt_at", "INTEGER", 0, 0),
    ("may_have_reached", "INTEGER", 0, 0),
];
const STEPS_WITH_METADATA: &[Column] = &[
    ("id", "TEXT", 1, 1),
    ("operation_id", "TEXT", 1, 0),
    ("position", "INTEGER", 1, 0),
    ("step_name", "TEXT", 1, 0),
    ("kind", "TEXT", 1, 0),
    ("intent", "TEXT", 0, 0),
    ("output", "BLOB", 0, 0),
    ("first_attempt_at", "INTEGER", 0, 0),
    ("may_have_reached", "INTEGER", 0, 0),
    ("created_at", "INTEGER", 1, 0),
    ("updated_at", "INTEGER", 1, 0),
];
const MARKER_V1: &[Column] = &[("version", "INTEGER", 1, 1)];
const MARKER_WITH_METADATA: &[Column] = &[
    ("id", "TEXT", 1, 1),
    ("version", "INTEGER", 1, 0),
    ("created_at", "INTEGER", 1, 0),
    ("updated_at", "INTEGER", 1, 0),
];

#[derive(sqlx::FromRow)]
struct ColumnRow {
    name: String,
    column_type: String,
    required: i64,
    primary_key: i64,
}

// PRAGMA shape inspection is dynamic; each table's statement is a literal.
const COLUMNS: [(&str, &str); 4] = [
    (
        RECEIPTS,
        "SELECT name, type AS column_type, \"notnull\" AS required, pk AS primary_key FROM pragma_table_info('_fleet_sync_migrations') ORDER BY cid",
    ),
    (
        OPERATIONS,
        "SELECT name, type AS column_type, \"notnull\" AS required, pk AS primary_key FROM pragma_table_info('_fleet_sync_operations') ORDER BY cid",
    ),
    (
        STEPS,
        "SELECT name, type AS column_type, \"notnull\" AS required, pk AS primary_key FROM pragma_table_info('_fleet_sync_steps') ORDER BY cid",
    ),
    (
        MARKER,
        "SELECT name, type AS column_type, \"notnull\" AS required, pk AS primary_key FROM pragma_table_info('_fleet_sync_schema') ORDER BY cid",
    ),
];

async fn has_columns(
    connection: &mut TursoConnection,
    table: &str,
    expected: &[&[Column]],
) -> Result<bool, StorageError> {
    let statement = COLUMNS
        .iter()
        .find(|(name, _)| *name == table)
        .map(|(_, statement)| *statement)
        .ok_or(StorageError::InvalidSchema)?;
    let actual = sqlx::query_as::<_, ColumnRow>(statement)
        .fetch_all(&mut *connection)
        .await
        .map_err(|_| StorageError::InvalidSchema)?;
    let expected: Vec<&Column> = expected.iter().flat_map(|part| part.iter()).collect();
    Ok(actual.len() == expected.len()
        && actual.iter().zip(expected).all(
            |(actual, (name, column_type, required, primary_key))| {
                actual.name == *name
                    && actual.column_type.eq_ignore_ascii_case(column_type)
                    && actual.required == *required
                    && actual.primary_key == *primary_key
            },
        ))
}

/// The journal's shape among the store's tables, or `InvalidSchema` for any other.
pub(super) async fn journal_shape(
    connection: &mut TursoConnection,
    tables: &BTreeSet<&str>,
) -> Result<JournalShape, StorageError> {
    let present = [RECEIPTS, OPERATIONS, STEPS, MARKER].map(|table| tables.contains(table));
    let shape = match present {
        [false, false, false, false] => return Ok(JournalShape::Absent),
        [true, true, true, true] => {
            if has_columns(connection, RECEIPTS, &[STANDARD_RECEIPTS]).await?
                && has_columns(connection, OPERATIONS, &[OPERATIONS_V1]).await?
                && has_columns(connection, STEPS, &[STEPS_V1]).await?
                && has_columns(connection, MARKER, &[MARKER_V1]).await?
            {
                JournalShape::MarkerV1
            } else if has_columns(connection, RECEIPTS, &[EXTENDED_RECEIPTS]).await?
                && has_columns(connection, OPERATIONS, &[OPERATIONS_V1, OPERATION_METADATA]).await?
                && has_columns(connection, STEPS, &[STEPS_WITH_METADATA]).await?
                && has_columns(connection, MARKER, &[MARKER_WITH_METADATA]).await?
            {
                JournalShape::ExtendedReceipts
            } else {
                return Err(StorageError::InvalidSchema);
            }
        }
        [true, true, true, false] => {
            let standard = has_columns(connection, RECEIPTS, &[STANDARD_RECEIPTS]).await?
                && has_columns(connection, OPERATIONS, &[OPERATIONS_V1, OPERATION_METADATA])
                    .await?
                && has_columns(connection, STEPS, &[STEPS_WITH_METADATA]).await?;
            if !standard {
                return Err(StorageError::InvalidSchema);
            }
            JournalShape::Standard
        }
        _ => return Err(StorageError::InvalidSchema),
    };
    Ok(shape)
}

/// After the pinned library's journal migration: exactly the standard shape.
pub(super) async fn require_standard(connection: &mut TursoConnection) -> Result<(), StorageError> {
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_schema WHERE type = 'table' AND name IN ('_fleet_sync_migrations', '_fleet_sync_operations', '_fleet_sync_steps', '_fleet_sync_schema')",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(|_| StorageError::InvalidSchema)?;
    let tables: BTreeSet<&str> = names.iter().map(String::as_str).collect();
    match journal_shape(connection, &tables).await? {
        JournalShape::Standard => Ok(()),
        _ => Err(StorageError::InvalidSchema),
    }
}
