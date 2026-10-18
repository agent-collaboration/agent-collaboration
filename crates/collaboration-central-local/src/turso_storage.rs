use crate::LocalResult;
use collaboration_central_core::*;
use collaboration_central_core::{
    BoundStatement, BoundValue, IdBatch, RegistryTime, SqlRow, SqlTx, StoreError, WireInput, step,
};
use std::{
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use turso_core::{
    Connection, Database, IO, LimboError, Numeric, OpenOptions, PlatformIO, SqliteDialect,
    StepResult, Value,
};
pub(crate) struct TursoStorage {
    database_path: PathBuf,
    #[cfg(feature = "conformance")]
    full_after_writes: Option<u64>,
    io: Arc<dyn IO>,
    _database: Arc<Database>,
    connection: Arc<Connection>,
}
fn unavailable(error: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable {
        message: error.to_string(),
    }
}
fn engine_error(error: LimboError) -> StoreError {
    match error {
        LimboError::DatabaseFull => StoreError::Full,
        LimboError::Constraint(message) | LimboError::ForeignKeyConstraint(message) => {
            StoreError::Constraint { message }
        }
        error => unavailable(error),
    }
}
impl TursoStorage {
    pub(crate) fn open(path: &Path) -> LocalResult<Self> {
        let io: Arc<dyn IO> = Arc::new(PlatformIO::new().map_err(engine_error)?);
        let database = Database::open(
            io.clone(),
            path.to_str().ok_or("object DB path must be UTF-8")?,
            OpenOptions::new(Arc::new(SqliteDialect)),
        )
        .map_err(engine_error)?;
        let connection = database.connect().map_err(engine_error)?;
        let storage = Self {
            database_path: path.to_owned(),
            #[cfg(feature = "conformance")]
            full_after_writes: None,
            io,
            _database: database,
            connection,
        };
        Ok(storage)
    }
    #[cfg(feature = "conformance")]
    pub(crate) fn inject_full(&mut self, after_writes: u64) {
        self.full_after_writes = Some(after_writes);
    }
    #[cfg(feature = "conformance")]
    pub(crate) fn corrupt(
        &mut self,
        mutation: collaboration_central_conformance::StorageCorruption,
    ) -> LocalResult<()> {
        use collaboration_central_conformance::StorageCorruption;
        let statements = match mutation {
            StorageCorruption::RestoreUnheldRegistry { unheld, as_of } => {
                let project = BoundValue::Text(unheld.project.as_str().into());
                let mut statements = vec![
                    (
                        "DELETE FROM _registry_leases WHERE project=?",
                        vec![project.clone()],
                    ),
                    (
                        "DELETE FROM _registry_transfers WHERE project=?",
                        vec![project.clone()],
                    ),
                    (
                        "DELETE FROM _registry_history WHERE project=?",
                        vec![project.clone()],
                    ),
                    (
                        "DELETE FROM _registry_claim_rejections WHERE project=?",
                        vec![project.clone()],
                    ),
                    (
                        "INSERT INTO _registry_epoch_floor(project,epoch) VALUES(?,?) ON CONFLICT(project) DO UPDATE SET epoch=excluded.epoch",
                        vec![
                            project.clone(),
                            BoundValue::Text(unheld.import_epoch.value().to_string()),
                        ],
                    ),
                ];
                statements.push(("INSERT INTO _registry_unheld(project,record) VALUES(?,?) ON CONFLICT(project) DO UPDATE SET record=excluded.record",vec![project,BoundValue::Text(serde_json::to_string(&serde_json::json!({"unheld":unheld,"change":1,"asOf":as_of}))?)]));
                statements
            }
            StorageCorruption::RemovePieceReceipts => {
                vec![("DELETE FROM _copy_receipts", Vec::new())]
            }
            StorageCorruption::ArchiveCopy => {
                vec![("UPDATE _copy_state SET archived=1", Vec::new())]
            }
            StorageCorruption::RemoveCopyState => vec![("DELETE FROM _copy_state", Vec::new())],
            StorageCorruption::InvalidateCopyFence => {
                vec![("UPDATE _copy_state SET fence_epoch='invalid'", Vec::new())]
            }
            StorageCorruption::ForgetRegistryFloor { project } => vec![(
                "DELETE FROM _registry_epoch_floor WHERE project=?",
                vec![BoundValue::Text(project.as_str().into())],
            )],
        };
        self.run_sql("BEGIN IMMEDIATE", &[])?;
        let result = (|| {
            for (sql, parameters) in statements {
                self.run_sql(sql, &parameters)?;
            }
            self.run_sql("COMMIT", &[])
        })();
        if let Err(error) = result {
            self.run_sql("ROLLBACK", &[])?;
            return Err(error.into());
        }
        Ok(())
    }
    pub(crate) fn alarm_plan(&mut self, address: &ObjectAddress) -> LocalResult<Vec<FaceAlarm>> {
        Ok(read_object_alarm_plan(self, address)?)
    }
    pub(crate) fn transact(
        &mut self,
        input: WireInput,
        now: RegistryTime,
        ids: IdBatch,
    ) -> LocalResult<ObjectEffects> {
        let health = match &input {
            WireInput::Call { call_id, call, .. }
                if matches!(
                    call.as_ref(),
                    WireCall::Copy(CopyCall::Ops(CopyOpsCall::Health {}))
                ) =>
            {
                Some((call_id.clone(), self.storage_bytes()?))
            }
            _ => None,
        };
        self.run_sql("BEGIN IMMEDIATE", &[])?;
        let result = match health {
            Some((call_id, storage_bytes)) => CopyKind::step(
                self,
                CopyInput::Health {
                    call_id,
                    storage_bytes,
                },
                now,
                ids,
            )
            .map(CopyKind::pack),
            None => step(self, input, now, ids),
        };
        #[cfg(feature = "conformance")]
        {
            self.full_after_writes = None;
        }
        match result {
            Ok(effects) => match self.run_sql("COMMIT", &[]) {
                Ok(_) => Ok(effects),
                Err(error) => {
                    self.run_sql("ROLLBACK", &[])?;
                    Err(error.into())
                }
            },
            Err(error) => {
                self.run_sql("ROLLBACK", &[])?;
                if let CentralError::CopyMigrationPending { failure, .. } = &error
                    && self.run_sql("BEGIN IMMEDIATE", &[]).is_ok()
                {
                    let recorded = record_copy_migration_failure(self, failure, now);
                    if recorded.is_err() || self.run_sql("COMMIT", &[]).is_err() {
                        let _ = self.run_sql("ROLLBACK", &[]);
                        eprintln!("copy migration failure diagnostic did not commit");
                    }
                }
                let effects = error.copy_effects_after_rollback();
                if effects.is_empty() {
                    Err(error.into())
                } else {
                    Ok(ObjectEffects::Copy(effects))
                }
            }
        }
    }
    /// Native accounting is the copy's main file plus its WAL, excluding SHM.
    /// It is physical file usage, not Cloudflare's logical databaseSize metric.
    fn storage_bytes(&self) -> LocalResult<u64> {
        let mut wal = self.database_path.as_os_str().to_owned();
        wal.push("-wal");
        let mut size = 0_u64;
        for path in [&self.database_path, &PathBuf::from(wal)] {
            match std::fs::metadata(path) {
                Ok(metadata) => {
                    size = size
                        .checked_add(metadata.len())
                        .ok_or("copy file size overflow")?
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(size)
    }
    fn run_sql(
        &mut self,
        sql: &str,
        parameters: &[BoundValue],
    ) -> Result<Vec<Vec<(String, BoundValue)>>, StoreError> {
        self.run_query_sql(sql, parameters, false)
    }
    fn run_query_sql(
        &mut self,
        sql: &str,
        parameters: &[BoundValue],
        reads_values: bool,
    ) -> Result<Vec<Vec<(String, BoundValue)>>, StoreError> {
        let mut prepared = self.connection.prepare(sql).map_err(engine_error)?;
        for (index, value) in parameters.iter().enumerate() {
            let index = NonZeroUsize::new(index + 1)
                .ok_or_else(|| unavailable("parameter index overflow"))?;
            let value = match value {
                BoundValue::Null => Value::Null,
                BoundValue::IntegerText(n) => Value::from_text(n.clone()),
                BoundValue::Real(n) => Value::from_f64(*n),
                BoundValue::Text(s) => Value::from_text(s.clone()),
                BoundValue::Blob(b) => Value::from_slice(b).map_err(unavailable)?,
            };
            prepared.bind_at(index, value).map_err(engine_error)?;
        }
        let names = (0..prepared.num_columns())
            .map(|index| prepared.get_column_name(index).into_owned())
            .collect::<Vec<_>>();
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut rows = Vec::new();
        loop {
            if Instant::now() >= deadline {
                return Err(unavailable("statement deadline exceeded"));
            }
            match prepared.step().map_err(engine_error)? {
                StepResult::Done => break,
                StepResult::Row => {
                    if !reads_values {
                        rows.push(Vec::new());
                        continue;
                    }
                    let row = prepared
                        .row()
                        .ok_or_else(|| unavailable("Row without values"))?;
                    let mut columns = Vec::new();
                    for (name, value) in names.iter().zip(row.get_values()) {
                        let value = match value {
                            Value::Null => BoundValue::Null,
                            Value::Numeric(Numeric::Integer(_)) => {
                                return Err(StoreError::Corrupt {
                                    field: "raw integer transport".into(),
                                });
                            }
                            Value::Numeric(Numeric::Float(n)) => BoundValue::Real(f64::from(*n)),
                            Value::Text(s) => BoundValue::Text(s.as_str().into()),
                            Value::Blob(b) => BoundValue::Blob(b.to_vec()),
                        };
                        columns.push((name.clone(), value));
                    }
                    if columns.len() != names.len() {
                        return Err(unavailable("row/column cardinality"));
                    }
                    rows.push(columns);
                }
                StepResult::IO | StepResult::Yield => self.io.step().map_err(engine_error)?,
                StepResult::Sleep { duration } => {
                    self.io.step().map_err(engine_error)?;
                    std::thread::sleep(
                        duration.min(deadline.saturating_duration_since(Instant::now())),
                    );
                }
                StepResult::Busy => return Err(unavailable("database busy")),
                StepResult::Interrupt => return Err(unavailable("statement interrupted")),
            }
        }
        Ok(rows)
    }
}
impl SqlTx for TursoStorage {
    fn execute(&mut self, statement: BoundStatement) -> Result<u64, StoreError> {
        #[cfg(feature = "conformance")]
        if let Some(remaining) = self.full_after_writes.as_mut() {
            if *remaining == 0 {
                return Err(StoreError::Full);
            }
            *remaining -= 1;
        }
        let before = self.connection.total_changes();
        self.run_sql(&statement.sql()?, statement.parameters())?;
        u64::try_from(self.connection.total_changes() - before)
            .map_err(|_| unavailable("changes counter regressed"))
    }
    fn query(&mut self, statement: BoundStatement) -> Result<Vec<SqlRow>, StoreError> {
        self.run_query_sql(
            &statement.sql()?,
            statement.parameters(),
            statement.reads_values(),
        )?
        .into_iter()
        .map(|row| statement.decode_row(row))
        .collect()
    }
}
