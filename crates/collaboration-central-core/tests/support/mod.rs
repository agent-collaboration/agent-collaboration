//! **Superseded (2026-10-16).** This is a third SQL engine (rusqlite) behind the synchronous
//! `SqlTx`. Core tests move to SQLx through our own drivers (requirements U70; `docs/specs/2026-10-16-central-one-core/program-design.md`).
#![allow(clippy::expect_used)]

use collaboration_central_core::{
    BoundStatement, BoundValue, CentralError, IdBatch, RegistryTime, SqlRow, SqlTx, StoreError,
    WireEffect, WireInput, migrate_copy_bookkeeping, step,
};
use rusqlite::{Connection, params_from_iter};

pub struct SqlTxFake {
    pub connection: Connection,
    pub fail_on_record_insert: Option<usize>,
    record_inserts: usize,
}

impl SqlTxFake {
    pub fn blank() -> Self {
        Self {
            connection: Connection::open_in_memory().expect("in-memory SQLite"),
            fail_on_record_insert: None,
            record_inserts: 0,
        }
    }

    pub fn new() -> Self {
        let mut store = Self::blank();
        store
            .connection
            .execute_batch("BEGIN IMMEDIATE")
            .expect("bootstrap begin");
        migrate_copy_bookkeeping(&mut store, RegistryTime(chrono::DateTime::UNIX_EPOCH))
            .expect("bootstrap");
        store
            .connection
            .execute_batch("COMMIT")
            .expect("bootstrap commit");
        store
    }

    pub fn run(
        &mut self,
        input: WireInput,
        now: RegistryTime,
    ) -> Result<Vec<WireEffect>, CentralError> {
        self.connection
            .execute_batch("BEGIN IMMEDIATE")
            .expect("begin");
        let result = step(
            self,
            input,
            now,
            IdBatch {
                registry_run: Some(
                    collaboration_central_core::RegistryRunId::new(
                        "019f0000-0000-7000-8000-000000090000",
                    )
                    .expect("registry run"),
                ),
                call_ids: (1..=16)
                    .map(|number| {
                        collaboration_central_core::ObjectCallId::new(format!(
                            "019f0000-0000-7000-8000-{number:012x}"
                        ))
                        .expect("test call ID")
                    })
                    .collect(),
            },
        );
        if result.is_ok() {
            self.connection.execute_batch("COMMIT").expect("commit");
        } else {
            self.connection.execute_batch("ROLLBACK").expect("rollback");
        }
        result.map(|effects| effects.into_trace())
    }

    pub fn record_ids(&self) -> Vec<i64> {
        let mut query = self
            .connection
            .prepare("SELECT id FROM records ORDER BY id")
            .expect("query");
        query
            .query_map([], |row| row.get(0))
            .expect("rows")
            .collect::<rusqlite::Result<Vec<i64>>>()
            .expect("ids")
    }
}

fn stored_values(parameters: &[BoundValue]) -> Vec<rusqlite::types::Value> {
    parameters
        .iter()
        .map(|value| match value {
            BoundValue::Null => rusqlite::types::Value::Null,
            BoundValue::IntegerText(value) => rusqlite::types::Value::Text(value.clone()),
            BoundValue::Real(value) => rusqlite::types::Value::Real(*value),
            BoundValue::Text(value) => rusqlite::types::Value::Text(value.clone()),
            BoundValue::Blob(value) => rusqlite::types::Value::Blob(value.clone()),
        })
        .collect()
}

fn storage_error(error: rusqlite::Error) -> StoreError {
    StoreError::Constraint {
        message: error.to_string(),
    }
}

impl SqlTx for SqlTxFake {
    fn execute(&mut self, statement: BoundStatement) -> Result<u64, StoreError> {
        let sql = statement.sql()?;
        if sql.starts_with("INSERT INTO \"records\"") {
            self.record_inserts += 1;
            if self.fail_on_record_insert == Some(self.record_inserts) {
                return Err(StoreError::Full);
            }
        }
        self.connection
            .execute(
                &sql,
                params_from_iter(stored_values(statement.parameters())),
            )
            .map(|count| count as u64)
            .map_err(storage_error)
    }

    fn query(&mut self, statement: BoundStatement) -> Result<Vec<SqlRow>, StoreError> {
        let mut prepared = self
            .connection
            .prepare(&statement.sql()?)
            .map_err(storage_error)?;
        let names = prepared
            .column_names()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut cursor = prepared
            .query(params_from_iter(stored_values(statement.parameters())))
            .map_err(storage_error)?;
        let mut rows = Vec::new();
        while let Some(row) = cursor.next().map_err(storage_error)? {
            if !statement.reads_values() {
                rows.push(statement.decode_row(Vec::new())?);
                continue;
            }
            let mut columns = Vec::new();
            for (index, name) in names.iter().enumerate() {
                let value = match row.get_ref(index).map_err(storage_error)? {
                    rusqlite::types::ValueRef::Null => BoundValue::Null,
                    rusqlite::types::ValueRef::Integer(_) => {
                        return Err(StoreError::Corrupt {
                            field: "raw integer transport".into(),
                        });
                    }
                    rusqlite::types::ValueRef::Real(value) => BoundValue::Real(value),
                    rusqlite::types::ValueRef::Text(value) => {
                        BoundValue::Text(String::from_utf8(value.to_vec()).expect("UTF-8"))
                    }
                    rusqlite::types::ValueRef::Blob(value) => BoundValue::Blob(value.to_vec()),
                };
                columns.push((name.clone(), value));
            }
            rows.push(statement.decode_row(columns)?);
        }
        Ok(rows)
    }
}
