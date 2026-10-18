use crate::copy_storage::query_statement;
use collaboration_replication_protocol::{
    ReplicatedTransaction, RowChange, SchemaIdentity, SqlValue,
};
use collaboration_replication_protocol::{SchemaEntry, SchemaObjectKind};

use crate::copy_storage::{statement, text, unsigned};
use crate::{CentralError, CopyStatus, SqlTx, StoreError};

pub(crate) fn quoted_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

#[derive(Clone, Debug)]
pub(crate) struct TableColumn {
    pub name: String,
    pub key_order: i64,
}

pub(crate) fn table_columns(
    tx: &mut dyn SqlTx,
    table: &str,
) -> Result<Vec<TableColumn>, CentralError> {
    if collaboration_replication_protocol::reserved_namespace(table) {
        return Err(CentralError::InvalidInput {
            message: "replicated table is reserved".into(),
        });
    }
    let rows = tx.query(query_statement(
        "SELECT name, pk FROM pragma_table_info(?)",
        vec![text(table)],
        ["name", "pk"],
    ))?;
    let columns = rows
        .iter()
        .map(|row| {
            Ok(TableColumn {
                name: row.text("name")?.into(),
                key_order: row.integer("pk")?,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    if columns.is_empty() || !columns.iter().any(|column| column.key_order > 0) {
        return Err(CentralError::InvalidInput {
            message: "replicated table must have a declared primary key".into(),
        });
    }
    Ok(columns)
}

pub(crate) fn primary_columns(columns: &[TableColumn]) -> Vec<&TableColumn> {
    let mut keys: Vec<_> = columns
        .iter()
        .filter(|column| column.key_order > 0)
        .collect();
    keys.sort_by_key(|column| column.key_order);
    keys
}

fn apply_row(tx: &mut dyn SqlTx, row: &RowChange) -> Result<(), CentralError> {
    let (table, key) = match row {
        RowChange::Upsert { table, key, .. } | RowChange::Delete { table, key } => (table, key),
    };
    let columns = table_columns(tx, table)?;
    let keys = primary_columns(&columns);
    if key.len() != keys.len() || key.iter().any(|value| matches!(value, SqlValue::Null)) {
        return Err(CentralError::InvalidInput {
            message: "row key does not match the declared primary key".into(),
        });
    }
    let table_name = quoted_identifier(table);
    let key_names = keys
        .iter()
        .map(|column| quoted_identifier(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    match row {
        RowChange::Upsert { values, .. } => {
            if values.len() != columns.len() {
                return Err(CentralError::InvalidInput {
                    message: "row values do not match column count".into(),
                });
            }
            for (column, key_value) in keys.iter().zip(key) {
                let index = columns
                    .iter()
                    .position(|candidate| candidate.name == column.name)
                    .ok_or_else(|| StoreError::Corrupt {
                        field: "primary_key".into(),
                    })?;
                if values.get(index) != Some(key_value) {
                    return Err(CentralError::InvalidInput {
                        message: "row image and key disagree".into(),
                    });
                }
            }
            let names = columns
                .iter()
                .map(|column| quoted_identifier(&column.name))
                .collect::<Vec<_>>()
                .join(", ");
            let placeholders = columns.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            let updates = columns
                .iter()
                .filter(|column| column.key_order == 0)
                .map(|column| {
                    let name = quoted_identifier(&column.name);
                    format!("{name}=excluded.{name}")
                })
                .collect::<Vec<_>>();
            let on_conflict = if updates.is_empty() {
                "DO NOTHING".into()
            } else {
                format!("DO UPDATE SET {}", updates.join(", "))
            };
            tx.execute(statement(&format!("INSERT INTO {table_name}({names}) VALUES({placeholders}) ON CONFLICT({key_names}) {on_conflict}"), values.clone()))?;
        }
        RowChange::Delete { .. } => {
            let predicates = keys
                .iter()
                .map(|column| format!("{}=?", quoted_identifier(&column.name)))
                .collect::<Vec<_>>()
                .join(" AND ");
            tx.execute(statement(
                &format!("DELETE FROM {table_name} WHERE {predicates}"),
                key.clone(),
            ))?;
        }
    }
    Ok(())
}

pub(crate) fn schema_fingerprint(tx: &mut dyn SqlTx) -> Result<String, CentralError> {
    let rows = tx.query(query_statement(
        "SELECT type, name, sql FROM sqlite_schema WHERE sql IS NOT NULL",
        Vec::new(),
        ["type", "name", "sql"],
    ))?;
    let entries = rows
        .iter()
        .map(|row| {
            Ok(SchemaEntry {
                kind: SchemaObjectKind::try_from(row.text("type")?).map_err(|_| {
                    StoreError::Corrupt {
                        field: "schema_type".into(),
                    }
                })?,
                name: row.text("name")?.into(),
                sql: row.text("sql")?.into(),
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    collaboration_replication_protocol::schema_fingerprint(&entries).map_err(|error| {
        CentralError::Encoding {
            message: error.to_string(),
        }
    })
}

pub(crate) fn apply_transactions(
    tx: &mut dyn SqlTx,
    state: &mut CopyStatus,
    transactions: &[ReplicatedTransaction],
) -> Result<(), CentralError> {
    tx.execute(statement("PRAGMA defer_foreign_keys=ON", Vec::new()))?;
    for transaction in transactions {
        match transaction {
            ReplicatedTransaction::Rows { rows, .. } => {
                for row in rows {
                    apply_row(tx, row)?;
                }
            }
            ReplicatedTransaction::SchemaStep {
                migration_version,
                statements,
                ..
            } => {
                if *migration_version <= state.schema.version {
                    return Err(CentralError::InvalidInput {
                        message: "schema version must advance".into(),
                    });
                }
                for (index, sql) in statements.iter().enumerate() {
                    tx.execute(statement(sql, Vec::new()))?;
                    let index = i64::try_from(index).map_err(|_| CentralError::InvalidInput {
                        message: "too many schema statements".into(),
                    })?;
                    tx.execute(statement("INSERT INTO _copy_schema_steps(migration_version, statement_index, statement) VALUES(?, ?, ?)", vec![unsigned(*migration_version), SqlValue::Integer(index), text(sql.clone())]))?;
                }
                state.schema = SchemaIdentity::new(*migration_version, schema_fingerprint(tx)?)
                    .map_err(|_| StoreError::Corrupt {
                        field: "schema_fingerprint".into(),
                    })?;
            }
        }
    }
    if !tx
        .query(crate::BoundStatement::existence(
            "PRAGMA foreign_key_check",
            Vec::new(),
        ))?
        .is_empty()
    {
        return Err(StoreError::Constraint {
            message: "replicated transaction fails foreign key check".into(),
        }
        .into());
    }
    Ok(())
}
