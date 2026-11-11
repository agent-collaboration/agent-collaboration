//! Narrow scratch-only SQL exception from Lead01a1c0e4; no project-row SQL.
use super::{ObservationError, check_sealed, seed_snapshot::check_scratch, source_hash};
use board_fleet_sync::BoardProjectSchema;
use fleet_sync_writer::ReplicatedDatabase;
use runtime_primitives::{Clock, IdSource, SystemClock, UuidV7Ids, WakeFailure, WakeScheduler};
use serde::Serialize;
use sqlx::{ConnectOptions, Connection, Row};
use sqlx_turso::{Turso, TursoConnectOptions, TursoConnection};
use std::sync::Arc;
use std::{ffi::OsString, path::Path};

struct LedgerHost;
impl Clock for LedgerHost {
    fn now(&self) -> runtime_primitives::SyncTime {
        SystemClock.now()
    }
}
impl IdSource for LedgerHost {
    fn new_id(&self) -> durable_steps::Uuid {
        UuidV7Ids.new_id()
    }
}
impl WakeScheduler for LedgerHost {
    async fn wake_at(&self, _: runtime_primitives::SyncTime) -> Result<(), WakeFailure> {
        Err(WakeFailure::Unavailable {
            message: "ledger inspection starts no tasks".to_owned(),
        })
    }
}
impl durable_steps::Host for LedgerHost {
    async fn dispatch<C: durable_steps::CallTarget>(
        &self,
        _: C,
        _: C::Request,
        _: durable_steps::CallId,
    ) -> Result<C::Answer, durable_steps::CallFailure> {
        Err(durable_steps::CallFailure::NotSent {
            reason: durable_steps::NotSentReason::Unreachable { last_seen: None },
        })
    }
}
pub(super) async fn adopt(
    writer: &Arc<ReplicatedDatabase<BoardProjectSchema>>,
) -> Result<(), ObservationError> {
    // The native runtime uses this same public constructor. It owns the capture-off
    // transaction; no engine task, scheduler or session is started by this probe.
    let journal = writer
        .journal_store(Arc::new(LedgerHost))
        .await
        .map_err(failure)?;
    drop(journal);
    Ok(())
}

const COLUMNS: &str = "PRAGMA table_info(_fleet_sync_migrations)";
const RECEIPTS: &str = "SELECT version,description,CAST(installed_on AS TEXT) AS installed_on,CAST(success AS TEXT) AS success,typeof(success) AS success_storage_class,hex(checksum) AS checksum,execution_time FROM _fleet_sync_migrations ORDER BY version";
const LEGACY: &str =
    "SELECT COUNT(*) AS count FROM sqlite_schema WHERE type='table' AND name='_fleet_sync_schema'";
const WINDOW: &str = "SELECT record -> '$.start' AS start,record -> '$.retainedAfter' AS retained_after,record -> '$.retainedChangeId' AS retained_change_id,record -> '$.retainedLayout' AS retained_layout FROM _fleet_sync_writer WHERE singleton=1";
const TAMPER: &str = "UPDATE _fleet_sync_migrations SET checksum=x'00' WHERE version=(SELECT MIN(version) FROM _fleet_sync_migrations)";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Column {
    cid: i64,
    name: String,
    storage_type: String,
    not_null: i64,
    default_value: Option<String>,
    primary_key: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    version: i64,
    description: String,
    installed_on: String,
    success: String,
    success_storage_class: String,
    checksum: String,
    execution_time: i64,
}
fn failure(error: impl std::fmt::Display) -> ObservationError {
    ObservationError::Read(error.to_string())
}
async fn inspect(connection: &mut TursoConnection) -> Result<serde_json::Value, ObservationError> {
    let columns = sqlx::query::<Turso>(COLUMNS)
        .fetch_all(&mut *connection)
        .await
        .map_err(failure)?
        .into_iter()
        .map(|row| {
            Ok(Column {
                cid: row.try_get("cid")?,
                name: row.try_get("name")?,
                storage_type: row.try_get("type")?,
                not_null: row.try_get("notnull")?,
                default_value: row.try_get("dflt_value")?,
                primary_key: row.try_get("pk")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(failure)?;
    let receipts = sqlx::query::<Turso>(RECEIPTS)
        .fetch_all(&mut *connection)
        .await
        .map_err(failure)?
        .into_iter()
        .map(|row| {
            Ok(Receipt {
                version: row.try_get("version")?,
                description: row.try_get("description")?,
                installed_on: row.try_get("installed_on")?,
                success: row.try_get("success")?,
                success_storage_class: row.try_get("success_storage_class")?,
                checksum: row.try_get("checksum")?,
                execution_time: row.try_get("execution_time")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(failure)?;
    let legacy = sqlx::query::<Turso>(LEGACY)
        .fetch_one(&mut *connection)
        .await
        .map_err(failure)?
        .try_get::<i64, _>("count")
        .map_err(failure)?;
    let row = sqlx::query::<Turso>(WINDOW)
        .fetch_one(&mut *connection)
        .await
        .map_err(failure)?;
    let window = serde_json::json!({
        "startRaw": row.try_get::<String, _>("start").map_err(failure)?,
        "retainedAfterRaw": row.try_get::<String, _>("retained_after").map_err(failure)?,
        "retainedChangeIdRaw": row.try_get::<String, _>("retained_change_id").map_err(failure)?,
        "retainedLayoutRaw": row.try_get::<String, _>("retained_layout").map_err(failure)?,
    });
    Ok(serde_json::json!({
        "kind": "ledgerInspection", "columns": columns, "receipts": receipts,
        "legacySchemaPresent": legacy != 0, "window": window,
        "statements": [
            {"sql": COLUMNS, "result": columns}, {"sql": RECEIPTS, "result": receipts},
            {"sql": LEGACY, "result": legacy}, {"sql": WINDOW, "result": window},
        ]
    }))
}

pub(super) async fn run(arguments: &[OsString]) -> Result<(), ObservationError> {
    let [mode, scratch, untouched] = arguments else {
        return Err(ObservationError::InvalidInput);
    };
    let scratch = Path::new(scratch);
    let untouched = Path::new(untouched);
    check_sealed(untouched)?;
    check_scratch(scratch, untouched)?;
    let before = source_hash(scratch)?;
    let sealed_before = source_hash(untouched)?;
    let tamper = mode == "--tamper-ledger-checksum";
    let mut connection = TursoConnectOptions::new()
        .filename(scratch)
        .create_if_missing(false)
        .read_only(!tamper)
        .connect()
        .await
        .map_err(failure)?;
    let result = if tamper {
        let result = sqlx::query::<Turso>(TAMPER)
            .execute(&mut connection)
            .await
            .map_err(failure)?;
        if result.rows_affected() != 1 {
            return Err(failure(
                "checksum mutation did not affect exactly one receipt",
            ));
        }
        serde_json::json!({"kind": "ledgerChecksumTampered", "statements": [{"sql": TAMPER, "rowsAffected": result.rows_affected()}]})
    } else {
        inspect(&mut connection).await?
    };
    connection.close().await.map_err(failure)?;
    let after = source_hash(scratch)?;
    if !tamper && before != after {
        return Err(ObservationError::SourceChanged);
    }
    let sealed_after = source_hash(untouched)?;
    if sealed_before != sealed_after {
        return Err(ObservationError::SourceChanged);
    }
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "observation": result, "scratchBeforeSha256": before, "scratchAfterSha256": after,
            "untouchedBeforeSha256": sealed_before, "untouchedAfterSha256": sealed_after,
        }))
        .map_err(failure)?
    );
    Ok(())
}
