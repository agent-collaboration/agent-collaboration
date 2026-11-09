//! Rebuilds one candidate object in a private in-memory Turso database: its
//! tables, then every row with its exact storage class, then its indexes and
//! any other schema objects, so unique indexes check the rows and nothing
//! fires while they load.

use crate::candidate_image::{ObjectImage, TypedValue};
// The candidate's own DDL runs only in this private in-memory database, and
// each INSERT names its table and columns as quoted identifiers with every
// value bound, so both are asserted SQL-safe.
use sqlx_core::{
    connection::ConnectOptions, query::query, raw_sql::raw_sql, sql_str::AssertSqlSafe,
};
use sqlx_turso::{Turso, TursoConnectOptions, TursoConnection};

#[derive(Debug, thiserror::Error)]
pub enum RebuildFailure {
    #[error("schema {name}: {source}")]
    Schema {
        name: String,
        #[source]
        source: sqlx_core::Error,
    },
    #[error("{table} row {row}: {reason}")]
    Row {
        table: String,
        row: usize,
        reason: String,
    },
    #[error("open: {0}")]
    Open(#[source] sqlx_core::Error),
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn blob(hex: &str) -> Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) {
        return Err("blob hex has odd length".into());
    }
    (0..hex.len())
        .step_by(2)
        .map(|at| {
            hex.get(at..at + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| "blob is not hex".to_owned())
        })
        .collect()
}

pub async fn rebuild(image: &ObjectImage) -> Result<TursoConnection, RebuildFailure> {
    let mut connection = TursoConnectOptions::new()
        .connect()
        .await
        .map_err(RebuildFailure::Open)?;
    for entry in image.schema.iter().filter(|entry| entry.kind == "table") {
        raw_sql(AssertSqlSafe(entry.sql.clone()))
            .execute(&mut connection)
            .await
            .map_err(|source| RebuildFailure::Schema {
                name: entry.name.clone(),
                source,
            })?;
    }
    // Rows load in table-creation order, so a row's foreign keys already exist.
    if let Some(table) = image.tables.keys().find(|table| {
        !image
            .schema
            .iter()
            .any(|entry| entry.kind == "table" && &entry.name == *table)
    }) {
        return Err(RebuildFailure::Row {
            table: table.clone(),
            row: 0,
            reason: "rows for a table the schema doesn't create".into(),
        });
    }
    for entry in image.schema.iter().filter(|entry| entry.kind == "table") {
        let table = &entry.name;
        let Some(rows) = image.tables.get(table) else {
            continue;
        };
        let columns = rows
            .columns
            .iter()
            .map(|column| quoted(column))
            .collect::<Vec<_>>();
        let placeholders = vec!["?"; columns.len()].join(", ");
        let statement = format!(
            "INSERT INTO {} ({}) VALUES ({placeholders})",
            quoted(table),
            columns.join(", ")
        );
        for (index, row) in rows.rows.iter().enumerate() {
            let failure = |reason: String| RebuildFailure::Row {
                table: table.clone(),
                row: index,
                reason,
            };
            if row.len() != columns.len() {
                return Err(failure(format!(
                    "{} values for {} columns",
                    row.len(),
                    columns.len()
                )));
            }
            let mut insert = query::<Turso>(AssertSqlSafe(statement.clone()));
            for value in row {
                insert = match value {
                    TypedValue::Null => insert.bind(None::<String>),
                    TypedValue::Integer(text) => insert.bind(
                        text.parse::<i64>()
                            .map_err(|_| failure(format!("integer {text} is not an i64")))?,
                    ),
                    TypedValue::Real(number) => insert.bind(*number),
                    TypedValue::Text(text) => insert.bind(text.clone()),
                    TypedValue::Blob(hex) => insert.bind(blob(hex).map_err(&failure)?),
                };
            }
            insert
                .execute(&mut connection)
                .await
                .map_err(|error| failure(error.to_string()))?;
        }
    }
    for entry in image.schema.iter().filter(|entry| entry.kind != "table") {
        raw_sql(AssertSqlSafe(entry.sql.clone()))
            .execute(&mut connection)
            .await
            .map_err(|source| RebuildFailure::Schema {
                name: entry.name.clone(),
                source,
            })?;
    }
    Ok(connection)
}
