use crate::copy_storage::query_statement;
use collaboration_replication_protocol::{
    ProjectCopyRead, ReadPage, SessionAnswer, SessionRefusal, SnapshotCursor, SnapshotPage,
};

use crate::copy_storage::load_copy;
use crate::row_apply::{primary_columns, quoted_identifier, table_columns};
use crate::{CentralError, RegistryTime, SqlTx, StoreError};

const PAGE_SIZE: usize = 256;

pub(crate) fn snapshot_page(
    tx: &mut dyn SqlTx,
    cursor: Option<&SnapshotCursor>,
    now: RegistryTime,
) -> Result<SessionAnswer, CentralError> {
    let Some(state) = load_copy(tx)? else {
        return Ok(SessionAnswer::Refused(SessionRefusal::NotInitialized));
    };
    if cursor.is_some_and(|cursor| cursor.position != state.acked) {
        return Ok(SessionAnswer::ReadPage(ReadPage::ProjectCopy(
            ProjectCopyRead::Moved {
                position: state.acked,
            },
        )));
    }
    let tables = tx.query(query_statement(
        "SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name",
        Vec::new(),
        ["name"],
    ))?;
    let names = tables
        .iter()
        .map(|row| row.text("name").map(str::to_owned))
        .collect::<Result<Vec<_>, StoreError>>()?
        .into_iter()
        .filter(|name| !collaboration_replication_protocol::reserved_namespace(name))
        .collect::<Vec<_>>();
    let table_index = match cursor {
        Some(cursor) => names
            .iter()
            .position(|table| table == &cursor.table)
            .ok_or_else(|| CentralError::InvalidInput {
                message: "snapshot cursor table unavailable".into(),
            })?,
        None => 0,
    };
    let table = names.get(table_index).cloned().unwrap_or_default();
    let mut stored_steps = tx
        .query(query_statement(
            "SELECT migration_version, statement_index, statement FROM _copy_schema_steps",
            Vec::new(),
            ["migration_version", "statement_index", "statement"],
        ))?
        .iter()
        .map(|row| {
            Ok((
                row.unsigned("migration_version")?,
                row.integer("statement_index")?,
                row.text("statement")?.to_owned(),
            ))
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    stored_steps.sort_by_key(|(version, index, _)| (*version, *index));
    let schema_steps = stored_steps.into_iter().map(|(_, _, sql)| sql).collect();
    let mut rows = Vec::new();
    let mut next = None;
    if !table.is_empty() {
        let columns = table_columns(tx, &table)?;
        let keys = primary_columns(&columns);
        let names_sql = columns
            .iter()
            .map(|column| quoted_identifier(&column.name))
            .collect::<Vec<_>>()
            .join(", ");
        let keys_sql = keys
            .iter()
            .map(|column| quoted_identifier(&column.name))
            .collect::<Vec<_>>()
            .join(", ");
        let after = cursor
            .map(|cursor| cursor.after_key.as_slice())
            .unwrap_or_default();
        if !after.is_empty() && after.len() != keys.len() {
            return Err(CentralError::InvalidInput {
                message: "snapshot cursor key shape mismatch".into(),
            });
        }
        let predicate = if after.is_empty() {
            String::new()
        } else {
            let placeholders = after.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            format!(" WHERE ({keys_sql}) > ({placeholders})")
        };
        let result = tx.query(query_statement(
            &format!(
                "SELECT {names_sql} FROM {}{predicate} ORDER BY {keys_sql} LIMIT {}",
                quoted_identifier(&table),
                PAGE_SIZE + 1
            ),
            after.to_vec(),
            columns.iter().map(|column| column.name.as_str()),
        ))?;
        for row in result.iter().take(PAGE_SIZE) {
            rows.push(
                columns
                    .iter()
                    .map(|column| row.value(&column.name).cloned())
                    .collect::<Result<Vec<_>, StoreError>>()?,
            );
        }
        if result.len() > PAGE_SIZE {
            let last =
                result
                    .iter()
                    .take(PAGE_SIZE)
                    .next_back()
                    .ok_or_else(|| StoreError::Corrupt {
                        field: "snapshot_page".into(),
                    })?;
            next = Some(SnapshotCursor {
                position: state.acked,
                table: table.clone(),
                after_key: keys
                    .iter()
                    .map(|column| last.value(&column.name).cloned())
                    .collect::<Result<Vec<_>, StoreError>>()?,
            });
        } else if let Some(table) = names.get(table_index + 1) {
            next = Some(SnapshotCursor {
                position: state.acked,
                table: table.clone(),
                after_key: Vec::new(),
            });
        }
    }
    Ok(SessionAnswer::ReadPage(ReadPage::ProjectCopy(
        ProjectCopyRead::Page(Box::new(SnapshotPage {
            project: state.project,
            position: state.acked,
            schema: state.schema,
            schema_steps,
            table,
            rows,
            next,
            as_of: now.0,
        })),
    )))
}
