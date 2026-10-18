//! The same tolerant metadata read serves copy prelude, Health and Migrate.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use turso_parser::{
    ast::{Cmd, Stmt},
    parser::Parser,
};

use crate::copy_storage::query_statement;
use crate::{CentralError, SqlTx, StoreError};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CopyBookkeepingMetadata {
    Uninitialized,
    Unrecorded { shape_v1: bool },
    Recorded { version: u64 },
}

pub fn read_copy_bookkeeping_metadata(
    tx: &mut dyn SqlTx,
) -> Result<CopyBookkeepingMetadata, CentralError> {
    // SQLite prepares every referenced table even inside an unselected CASE
    // branch. Probe the schema before constructing any ledger query.
    let tables = tx
        .query(query_statement(
            "SELECT name,sql FROM sqlite_schema WHERE type='table' AND name GLOB '_copy_*' ORDER BY name",
            Vec::new(),
            ["name", "sql"],
        ))?
        .iter()
        .map(|row| Ok((row.text("name")?.to_owned(), row.text("sql")?.to_owned())))
        .collect::<Result<BTreeMap<_, _>, StoreError>>()?;
    if tables.contains_key("_copy_migrations") {
        let rows = tx.query(query_statement(
            "SELECT MAX(version) AS version FROM _copy_migrations",
            Vec::new(),
            ["version"],
        ))?;
        let version = rows
            .first()
            .ok_or_else(|| damaged("copy migration version"))?
            .integer("version")?;
        let version = u64::try_from(version)
            .ok()
            .filter(|version| *version > 0)
            .ok_or_else(|| damaged("copy migration version"))?;
        return Ok(CopyBookkeepingMetadata::Recorded { version });
    }
    if tables.keys().all(|name| name == "_copy_migration_failure") {
        return Ok(CopyBookkeepingMetadata::Uninitialized);
    }
    Ok(CopyBookkeepingMetadata::Unrecorded {
        shape_v1: legacy_shape_matches(&tables)?,
    })
}

fn legacy_shape_matches(tables: &BTreeMap<String, String>) -> Result<bool, CentralError> {
    let mut parser = Parser::new(include_bytes!("../migrations/0001_copy_storage.sql"));
    let mut recognized = BTreeSet::new();
    while let Some(expected) = parser
        .next_cmd()
        .map_err(|_| damaged("copy v1 migration SQL"))?
    {
        let Cmd::Stmt(Stmt::CreateTable { tbl_name, .. }) = &expected else {
            return Err(damaged("copy v1 migration statement").into());
        };
        let Some(actual_sql) = tables.get(tbl_name.name.as_str()) else {
            return Ok(false);
        };
        if !same_table_shape(&expected, actual_sql) {
            return Ok(false);
        }
        recognized.insert(tbl_name.name.as_str().to_owned());
    }
    let outbox = directory_outbox_schema()?;
    for (name, sql) in tables {
        if recognized.contains(name) || name == "_copy_migration_failure" {
            continue;
        }
        if name == "_copy_directory_outbox" && same_table_shape(&outbox, sql) {
            continue;
        }
        return Ok(false);
    }
    Ok(true)
}

fn same_table_shape(expected: &Cmd, actual_sql: &str) -> bool {
    let mut parser = Parser::new(actual_sql.as_bytes());
    let Ok(Some(mut actual)) = parser.next_cmd() else {
        return false;
    };
    if !matches!(parser.next_cmd(), Ok(None)) {
        return false;
    }
    let mut expected = expected.clone();
    // IF NOT EXISTS controls creation, not a stored table's shape. Engines may
    // omit it from sqlite_schema even though the old producer supplied it.
    for command in [&mut expected, &mut actual] {
        let Cmd::Stmt(Stmt::CreateTable { if_not_exists, .. }) = command else {
            return false;
        };
        *if_not_exists = false;
    }
    expected == actual
}
fn directory_outbox_schema() -> Result<Cmd, CentralError> {
    let mut parser = Parser::new(include_bytes!("../migrations/0012_copy_operations.sql"));
    let command = parser
        .next_cmd()
        .map_err(|_| damaged("copy outbox migration SQL"))?
        .ok_or_else(|| damaged("copy outbox migration SQL"))?;
    if !matches!(&command,Cmd::Stmt(Stmt::CreateTable { tbl_name,.. }) if tbl_name.name.as_str()=="_copy_directory_outbox")
    {
        return Err(damaged("copy outbox migration statement").into());
    }
    Ok(command)
}
pub(crate) fn directory_outbox_shape_matches(tx: &mut dyn SqlTx) -> Result<bool, CentralError> {
    let rows = tx.query(query_statement(
        "SELECT sql FROM sqlite_schema WHERE type='table' AND name='_copy_directory_outbox'",
        Vec::new(),
        ["sql"],
    ))?;
    if rows.len() != 1 {
        return Ok(false);
    }
    let row = rows.first().ok_or_else(|| damaged("copy outbox schema"))?;
    Ok(same_table_shape(
        &directory_outbox_schema()?,
        row.text("sql")?,
    ))
}

fn damaged(field: &str) -> StoreError {
    StoreError::Corrupt {
        field: field.into(),
    }
}
