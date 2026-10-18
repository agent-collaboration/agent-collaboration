use collaboration_central_core::*;
use collaboration_central_core::{
    BoundStatement, BoundValue, IdBatch, ObjectAddress, RegistryTime, SqlRow, SqlTx, StoreError,
    WireInput, step,
};
use serde::Deserialize;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use worker::{SqlStorage, SqlStorageValue, Storage, wasm_bindgen::JsValue};
/// Unknown platform scalar envelope, classified once before the core decodes
/// the storage-class tags and exact integer text its query plan declared.
#[derive(Deserialize)]
#[serde(transparent)]
struct PlatformScalar(#[serde(with = "serde_wasm_bindgen::preserve")] JsValue);
fn scalar(value: JsValue) -> Result<BoundValue, StoreError> {
    if value.is_null() {
        return Ok(BoundValue::Null);
    }
    if let Some(text) = value.as_string() {
        return Ok(BoundValue::Text(text));
    }
    if let Some(real) = value.as_f64() {
        return Ok(BoundValue::Real(real));
    }
    match SqlStorageValue::try_from(value).map_err(storage_error)? {
        SqlStorageValue::Blob(bytes) => Ok(BoundValue::Blob(bytes)),
        _ => Err(StoreError::Corrupt {
            field: "unexpected platform scalar".into(),
        }),
    }
}
fn storage_error(error: worker::Error) -> StoreError {
    let message = error.to_string();
    let full = matches!(&error,worker::Error::UnknownJsError{code:Some(code),..}if code=="SQLITE_FULL")
        || message.contains("database or disk is full")
        || message.contains("exceeded the storage limit");
    if full {
        StoreError::Full
    } else if message.contains("constraint failed") {
        StoreError::Constraint { message }
    } else {
        StoreError::Unavailable { message }
    }
}
pub(crate) struct CloudflareSqlTx {
    sql: SqlStorage,
    #[cfg(feature = "conformance")]
    pub full_after_writes: Option<u64>,
    #[cfg(feature = "conformance-faults")]
    pub panic_after_writes: Option<u64>,
}
#[derive(Default)]
pub(crate) struct TransactionFaults {
    pub full_after_writes: Option<u64>,
    #[cfg(feature = "conformance-faults")]
    pub panic_after_writes: Option<u64>,
}
impl CloudflareSqlTx {
    fn bindings(statement: &BoundStatement) -> Vec<SqlStorageValue> {
        statement
            .parameters()
            .iter()
            .map(|value| match value {
                BoundValue::Null => SqlStorageValue::Null,
                BoundValue::IntegerText(value) | BoundValue::Text(value) => {
                    SqlStorageValue::String(value.clone())
                }
                BoundValue::Real(value) => SqlStorageValue::Float(*value),
                BoundValue::Blob(value) => SqlStorageValue::Blob(value.clone()),
            })
            .collect()
    }
}
impl SqlTx for CloudflareSqlTx {
    fn execute(&mut self, statement: BoundStatement) -> Result<u64, StoreError> {
        #[cfg(feature = "conformance")]
        if let Some(remaining) = self.full_after_writes.as_mut() {
            if *remaining == 0 {
                return Err(StoreError::Full);
            }
            *remaining -= 1;
        }
        let cursor = self
            .sql
            .exec(&statement.sql()?, Some(Self::bindings(&statement)))
            .map_err(storage_error)?;
        // Consume any result rows so each statement completes in the callback.
        cursor
            .to_array::<serde::de::IgnoredAny>()
            .map_err(storage_error)?;
        #[cfg(feature = "conformance-faults")]
        if let Some(remaining) = self.panic_after_writes.as_mut() {
            *remaining = remaining.saturating_sub(1);
            if *remaining == 0 {
                crate::conformance_faults::panic_inside_transaction();
            }
        }
        u64::try_from(cursor.rows_written()).map_err(|_| StoreError::Corrupt {
            field: "rows written".into(),
        })
    }
    fn query(&mut self, statement: BoundStatement) -> Result<Vec<SqlRow>, StoreError> {
        let cursor = self
            .sql
            .exec(&statement.sql()?, Some(Self::bindings(&statement)))
            .map_err(storage_error)?;
        if !statement.reads_values() {
            return cursor
                .to_array::<serde::de::IgnoredAny>()
                .map_err(storage_error)?
                .into_iter()
                .map(|_| statement.decode_row(Vec::new()))
                .collect();
        }
        cursor
            .to_array::<BTreeMap<String, PlatformScalar>>()
            .map_err(storage_error)?
            .into_iter()
            .map(|row| {
                let columns = row
                    .into_iter()
                    .map(|(name, value)| Ok((name, scalar(value.0)?)))
                    .collect::<Result<Vec<_>, StoreError>>()?;
                statement.decode_row(columns)
            })
            .collect()
    }
}
pub(crate) async fn storage_transaction(
    storage: &Storage,
    address: &ObjectAddress,
    input: WireInput,
    now: RegistryTime,
    ids: IdBatch,
    faults: TransactionFaults,
) -> worker::Result<ObjectEffects> {
    let sql = storage.sql();
    let health = match (address, &input) {
        (ObjectAddress::Copy(_), WireInput::Call { call_id, call, .. })
            if matches!(
                call.as_ref(),
                WireCall::Copy(CopyCall::Ops(CopyOpsCall::Health {}))
            ) =>
        {
            Some((
                call_id.clone(),
                crate::storage_measurement::storage_bytes(storage)?,
            ))
        }
        _ => None,
    };
    let outcome = Rc::new(RefCell::new(None));
    let written = outcome.clone();
    #[cfg(not(feature = "conformance"))]
    let _ = faults.full_after_writes;
    let transaction = storage
        .transaction(move |_transaction| async move {
            let mut tx = CloudflareSqlTx {
                sql,
                #[cfg(feature = "conformance")]
                full_after_writes: faults.full_after_writes,
                #[cfg(feature = "conformance-faults")]
                panic_after_writes: faults.panic_after_writes,
            };
            let result = match health {
                Some((call_id, storage_bytes)) => CopyKind::step(
                    &mut tx,
                    CopyInput::Health {
                        call_id,
                        storage_bytes,
                    },
                    now,
                    ids,
                )
                .map(CopyKind::pack),
                None => step(&mut tx, input, now, ids),
            };
            let failed = result.as_ref().err().map(ToString::to_string);
            *written.borrow_mut() = Some(result);
            if let Some(error) = failed {
                Err(worker::Error::RustError(error))
            } else {
                Ok(())
            }
        })
        .await;
    let result = outcome.borrow_mut().take().ok_or_else(|| {
        worker::Error::RustError("transaction callback returned no core result".into())
    })?;
    match (transaction, result) {
        (Ok(()), Ok(effects)) => Ok(effects),
        (Err(_), Err(error)) => {
            if let CentralError::CopyMigrationPending { failure, .. } = &error {
                let failure = failure.clone();
                let sql = storage.sql();
                let recorded = storage
                    .transaction(move |_transaction| async move {
                        let mut tx = CloudflareSqlTx {
                            sql,
                            #[cfg(feature = "conformance")]
                            full_after_writes: None,
                            #[cfg(feature = "conformance-faults")]
                            panic_after_writes: None,
                        };
                        record_copy_migration_failure(&mut tx, &failure, now)
                            .map_err(|error| worker::Error::RustError(error.to_string()))
                    })
                    .await;
                if recorded.is_err() {
                    worker::console_warn!("copy migration failure diagnostic did not commit");
                }
            }
            let effects = error.copy_effects_after_rollback();
            if effects.is_empty() {
                Err(worker::Error::RustError(error.to_string()))
            } else {
                Ok(ObjectEffects::Copy(effects))
            }
        }
        (Err(error), Ok(_)) => Err(error),
        (Ok(()), Err(_)) => Err(worker::Error::RustError(
            "transaction committed a failed core step".into(),
        )),
    }
}

pub(crate) fn alarm_plan(
    storage: &Storage,
    address: &ObjectAddress,
) -> worker::Result<Vec<FaceAlarm>> {
    let mut tx = CloudflareSqlTx {
        sql: storage.sql(),
        #[cfg(feature = "conformance")]
        full_after_writes: None,
        #[cfg(feature = "conformance-faults")]
        panic_after_writes: None,
    };
    read_object_alarm_plan(&mut tx, address)
        .map_err(|error| worker::Error::RustError(error.to_string()))
}
