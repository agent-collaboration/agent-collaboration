//! Read-only identity checks for the shipped ffced1c1 SQLite format. The manifest
//! records normalized sqlite_master definitions and SQLx SHA-384 migration hashes
//! from that commit's public BoardStore fixture; no legacy DDL is executed here.
use crate::{UpgradeError, import_error::invalid};
use serde::Deserialize;
use sqlx::{Row, SqliteConnection};

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

pub(crate) async fn validate_supported_schema(
    connection: &mut SqliteConnection,
) -> Result<(), UpgradeError> {
    let expected: SchemaIdentity =
        serde_json::from_str(include_str!("legacy_schema_identity.json"))?;
    let matches = matches_identity(connection, &expected)
        .await
        .map_err(|error| invalid("schema", error))?;
    if !matches {
        return Err(invalid(
            "schema",
            format!(
                "not the complete shipped {} schema, migration history and checkpoint",
                expected.source_commit,
            ),
        ));
    }
    Ok(())
}

async fn matches_identity(
    connection: &mut SqliteConnection,
    expected: &SchemaIdentity,
) -> Result<bool, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT name,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&mut *connection)
    .await?;
    let objects = rows
        .into_iter()
        .map(|row| {
            let sql: String = row.try_get("sql")?;
            Ok(SchemaObject {
                name: row.try_get("name")?,
                sql: sql.split_whitespace().collect::<Vec<_>>().join(" "),
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    if objects != expected.objects {
        return Ok(false);
    }
    let rows = sqlx::query(
        "SELECT version,description,success,hex(checksum) AS checksum FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(&mut *connection)
    .await?;
    let migrations = rows
        .into_iter()
        .map(|row| {
            Ok(MigrationIdentity {
                version: row.try_get("version")?,
                description: row.try_get("description")?,
                success: row.try_get("success")?,
                checksum: row.try_get("checksum")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    if migrations != expected.migrations {
        return Ok(false);
    }
    let valid_checkpoint: bool = sqlx::query_scalar(
        "SELECT count(*)=1 FROM activity_checkpoint WHERE singleton=1 AND last_sequence>=0 AND length(cursor_key)=32 AND (SELECT count(*) FROM activity_checkpoint)=1",
    )
    .fetch_one(&mut *connection)
    .await?;
    let foreign_key_violation = sqlx::query("PRAGMA foreign_key_check")
        .fetch_optional(connection)
        .await?;
    Ok(valid_checkpoint && foreign_key_violation.is_none())
}
