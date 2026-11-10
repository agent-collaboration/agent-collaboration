//! Read-only identity checks for the shipped ffced1c1 SQLite format. The manifest
//! records normalized sqlite_master definitions and SQLx SHA-384 migration hashes
//! from that commit's public BoardStore fixture; no legacy DDL is executed here.
use crate::{UpgradeError, import_error::invalid};
use rusqlite::Connection;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SchemaIdentity {
    source_commit: String,
    objects: Vec<SchemaObject>,
    migrations: Vec<MigrationIdentity>,
}

#[derive(Deserialize, PartialEq)]
struct SchemaObject {
    name: String,
    sql: String,
}

#[derive(Deserialize, PartialEq)]
struct MigrationIdentity {
    version: i64,
    description: String,
    success: i64,
    checksum: String,
}

pub(crate) fn validate_supported_schema(connection: &Connection) -> Result<(), UpgradeError> {
    let expected: SchemaIdentity =
        serde_json::from_str(include_str!("legacy_schema_identity.json"))?;
    let matches =
        matches_identity(connection, &expected).map_err(|error| invalid("schema", error))?;
    if !matches {
        return Err(invalid(
            "schema",
            format!(
                "not the complete shipped {} schema, migration history and checkpoint",
                expected.source_commit
            ),
        ));
    }
    Ok(())
}

fn matches_identity(connection: &Connection, expected: &SchemaIdentity) -> rusqlite::Result<bool> {
    let mut statement = connection.prepare(
        "SELECT name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let objects = statement
        .query_map([], |row| {
            let sql: String = row.get("sql")?;
            Ok(SchemaObject {
                name: row.get("name")?,
                sql: sql.split_whitespace().collect::<Vec<_>>().join(" "),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if objects != expected.objects {
        return Ok(false);
    }
    let mut statement = connection.prepare(
        "SELECT version,description,success,hex(checksum) AS checksum FROM _sqlx_migrations ORDER BY version",
    )?;
    let migrations = statement
        .query_map([], |row| {
            Ok(MigrationIdentity {
                version: row.get("version")?,
                description: row.get("description")?,
                success: row.get("success")?,
                checksum: row.get("checksum")?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if migrations != expected.migrations {
        return Ok(false);
    }
    let valid_checkpoint: bool = connection.query_row(
        "SELECT count(*)=1 FROM activity_checkpoint WHERE singleton=1 AND last_sequence>=0 AND length(cursor_key)=32 AND (SELECT count(*) FROM activity_checkpoint)=1",
        [],
        |row| row.get(0),
    )?;
    let mut foreign_keys = connection.prepare("PRAGMA foreign_key_check")?;
    Ok(valid_checkpoint && foreign_keys.query([])?.next()?.is_none())
}
