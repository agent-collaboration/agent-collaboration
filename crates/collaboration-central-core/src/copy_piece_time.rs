//! The timestamp of accepted pieces, including metadata retained by v1 copies.
use chrono::{DateTime, Utc};
use collaboration_replication_protocol::SqlValue;

use crate::copy_storage::{query_statement, statement, text};
use crate::{BoundStatement, CentralError, RegistryTime, SqlRow, SqlTx, StoreError};

fn corrupt() -> CentralError {
    StoreError::Corrupt {
        field: "last piece time".into(),
    }
    .into()
}
fn parse_piece_time(value: &SqlValue) -> Result<Option<DateTime<Utc>>, CentralError> {
    match value {
        SqlValue::Null => Ok(None),
        SqlValue::Text(value) => DateTime::parse_from_rfc3339(value)
            .map(|time| Some(time.with_timezone(&Utc)))
            .map_err(|_| corrupt()),
        _ => Err(corrupt()),
    }
}
fn has_rows(tx: &mut dyn SqlTx, table: &str) -> Result<bool, CentralError> {
    if !crate::registry_dispatch::has_table(tx, table)? {
        return Ok(false);
    }
    let table = crate::row_apply::quoted_identifier(table);
    Ok(!tx
        .query(BoundStatement::existence(
            format!("SELECT 1 FROM {table} LIMIT 1"),
            Vec::new(),
        ))?
        .is_empty())
}
fn read_legacy_piece_time(tx: &mut dyn SqlTx) -> Result<Option<DateTime<Utc>>, CentralError> {
    let mut rows: Vec<SqlRow> = Vec::new();
    // An import also writes _copy_summary, but isn't a replicated piece.
    if (has_rows(tx, "_copy_receipts")? || has_rows(tx, "_copy_part_receipts")?)
        && crate::registry_dispatch::has_table(tx, "_copy_summary")?
    {
        rows.extend(tx.query(query_statement(
            "SELECT acked_at AS recorded_at FROM _copy_summary WHERE singleton=1",
            Vec::new(),
            ["recorded_at"],
        ))?);
    }
    if crate::registry_dispatch::has_table(tx, "_copy_staged_parts")? {
        rows.extend(tx.query(query_statement(
            "SELECT staged_at AS recorded_at FROM _copy_staged_parts",
            Vec::new(),
            ["recorded_at"],
        ))?);
    }
    let mut latest = None;
    for row in rows {
        let time = parse_piece_time(row.value("recorded_at")?)?.ok_or_else(corrupt)?;
        latest = Some(latest.map_or(time, |old: DateTime<Utc>| old.max(time)));
    }
    Ok(latest)
}
pub(crate) fn read_piece_time(tx: &mut dyn SqlTx) -> Result<Option<DateTime<Utc>>, CentralError> {
    let columns = tx.query(query_statement(
        "SELECT name FROM pragma_table_info('_copy_state')",
        Vec::new(),
        ["name"],
    ))?;
    let mut recorded = false;
    for row in columns {
        if row.text("name")? == "last_piece_at" {
            recorded = true;
        }
    }
    if !recorded {
        return read_legacy_piece_time(tx);
    }
    let rows = tx.query(query_statement(
        "SELECT last_piece_at FROM _copy_state WHERE singleton=1",
        Vec::new(),
        ["last_piece_at"],
    ))?;
    rows.first()
        .map(|row| parse_piece_time(row.value("last_piece_at")?))
        .transpose()
        .map(Option::flatten)
}
pub(crate) fn record_new_piece_time(
    tx: &mut dyn SqlTx,
    now: RegistryTime,
) -> Result<(), CentralError> {
    write_piece_time(tx, Some(now.0))
}
pub(crate) fn backfill_piece_time(tx: &mut dyn SqlTx) -> Result<(), CentralError> {
    let time = read_legacy_piece_time(tx)?;
    write_piece_time(tx, time)
}
fn write_piece_time(tx: &mut dyn SqlTx, time: Option<DateTime<Utc>>) -> Result<(), CentralError> {
    tx.execute(statement(
        "UPDATE _copy_state SET last_piece_at=? WHERE singleton=1",
        vec![time.map_or(SqlValue::Null, |time| text(time.to_rfc3339()))],
    ))?;
    Ok(())
}
