//! Health joins public receiver inspection with central's tolerant own bookkeeping.
use crate::step_driver::{StepCallback, StepResult};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

use collaboration_central_admin_protocol::{
    CopyBookkeepingVersion, CopyHealth, CopyHealthRead, CopyMigrationFailureRecord, CopyStage,
    LiveCopyHealth, StagedPartHealth,
};
pub(crate) struct ReadCopyHealth {
    pub storage_bytes: u64,
}
fn readable<Value>(value: Result<Value, StoreError>) -> CopyHealthRead<Value> {
    match value {
        Ok(value) => CopyHealthRead::Known(value),
        Err(error) => CopyHealthRead::Unreadable {
            detail: error.to_string(),
        },
    }
}
fn time(at: runtime_primitives::SyncTime) -> Result<chrono::DateTime<chrono::Utc>, StoreError> {
    chrono::DateTime::from_timestamp_millis(at.unix_millis())
        .ok_or_else(|| StoreError::backend("stored health time is outside the display range"))
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadCopyHealth
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Driver>
        + sqlx_core::encode::Encode<'blob, Driver>
        + sqlx_core::decode::Decode<'blob, Driver>,
    for<'scalar> i64: sqlx_core::types::Type<Driver>
        + sqlx_core::encode::Encode<'scalar, Driver>
        + sqlx_core::decode::Decode<'scalar, Driver>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Driver> + sqlx_core::decode::Decode<'blob, Driver>,
    for<'scalar> Option<i64>:
        sqlx_core::types::Type<Driver> + sqlx_core::decode::Decode<'scalar, Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = CopyHealth;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            use fleet_project_copy::CopyInspection;
            let inspection = fleet_project_copy::inspect_copy(scope).await;
            if let CopyInspection::Retired { retired_at } = inspection {
                return Ok((
                    CopyHealth::Retired {
                        retired_at: time(retired_at)?,
                    },
                    Vec::new(),
                ));
            }
            let bookkeeping = readable(read_bookkeeping(scope).await);
            let migration_failures = readable(read_failures(scope).await);
            let (stage, copy, project_fingerprint, staged_parts) = match inspection {
                CopyInspection::Uninitialized => (
                    CopyHealthRead::Known(None),
                    CopyHealthRead::Known(None),
                    CopyHealthRead::Known(
                        fleet_protocol::schema_fingerprint(&[]).map_err(StoreError::backend)?,
                    ),
                    CopyHealthRead::Known(Vec::new()),
                ),
                CopyInspection::Live {
                    stage,
                    status,
                    fingerprint,
                    staged_parts,
                } => {
                    let mut parts = Vec::new();
                    for staged in staged_parts {
                        for part in staged.parts {
                            parts.push(StagedPartHealth {
                                transaction: staged.transaction_key,
                                index: part.index,
                                staged_at: time(part.staged_at)?,
                            });
                        }
                    }
                    (
                        CopyHealthRead::Known(Some(match stage {
                            fleet_project_copy::CopyStage::Starting => CopyStage::Starting,
                            fleet_project_copy::CopyStage::Seeding => CopyStage::Seeding,
                            fleet_project_copy::CopyStage::Following => CopyStage::Following,
                        })),
                        CopyHealthRead::Known(Some(status)),
                        CopyHealthRead::Known(fingerprint),
                        CopyHealthRead::Known(parts),
                    )
                }
                CopyInspection::Damaged { detail } => (
                    CopyHealthRead::Unreadable {
                        detail: detail.clone(),
                    },
                    CopyHealthRead::Unreadable {
                        detail: detail.clone(),
                    },
                    CopyHealthRead::Unreadable {
                        detail: detail.clone(),
                    },
                    CopyHealthRead::Unreadable { detail },
                ),
                CopyInspection::Retired { .. } => {
                    return Err(StoreError::backend(
                        "retired inspection escaped the health gate",
                    ));
                }
            };
            Ok((
                CopyHealth::Live(Box::new(LiveCopyHealth {
                    bookkeeping,
                    migration_failures,
                    stage,
                    copy,
                    project_fingerprint,
                    staged_parts,
                    storage_bytes: self.storage_bytes,
                })),
                Vec::new(),
            ))
        })
    }
}
async fn table_present<Scope: TransactionScope>(
    scope: &mut Scope,
    name: &str,
) -> Result<bool, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Scope::Database>
        + sqlx_core::encode::Encode<'blob, Scope::Database>
        + sqlx_core::decode::Decode<'blob, Scope::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Scope::Database>
        + sqlx_core::encode::Encode<'scalar, Scope::Database>
        + sqlx_core::decode::Decode<'scalar, Scope::Database>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Scope::Database> + sqlx_core::decode::Decode<'blob, Scope::Database>,
    for<'scalar> Option<i64>: sqlx_core::types::Type<Scope::Database>
        + sqlx_core::decode::Decode<'scalar, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    use crate::fleet_record_codec::required;
    for row in crate::generated_bootstrap::probe_migration_bookkeeping(scope)
        .await
        .map_err(StoreError::backend)?
    {
        if required(row.name, "migration table name")? == name {
            return Ok(true);
        }
    }
    Ok(false)
}
async fn read_bookkeeping<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<Vec<CopyBookkeepingVersion>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Scope::Database>
        + sqlx_core::encode::Encode<'blob, Scope::Database>
        + sqlx_core::decode::Decode<'blob, Scope::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Scope::Database>
        + sqlx_core::encode::Encode<'scalar, Scope::Database>
        + sqlx_core::decode::Decode<'scalar, Scope::Database>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Scope::Database> + sqlx_core::decode::Decode<'blob, Scope::Database>,
    for<'scalar> Option<i64>: sqlx_core::types::Type<Scope::Database>
        + sqlx_core::decode::Decode<'scalar, Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let supported = fleet_project_copy::RECEIVER_MIGRATOR
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .map(|migration| u64::try_from(migration.version).map_err(StoreError::backend))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .max()
        .ok_or_else(|| StoreError::backend("receiver corpus is empty"))?;
    let present = table_present(scope, "central_schema_migrations").await?;
    let mut stored = None;
    let rows = if present {
        crate::generated_bootstrap::read_migration_history(scope, "fleet-sync-receiver".into())
            .await
            .map_err(StoreError::backend)?
    } else {
        Vec::new()
    };
    for row in rows {
        crate::migration_bookkeeping::validate_history_times(&row)?;
        crate::fleet_record_codec::check_private_id(
            &crate::fleet_record_codec::required(row.row_id, "migration row identity")?,
            "migration row identity",
        )?;
        let version = u64::try_from(
            row.version
                .ok_or_else(|| StoreError::backend("missing migration version"))?,
        )
        .map_err(StoreError::backend)?;
        stored = Some(stored.map_or(version, |previous: u64| previous.max(version)));
    }
    Ok(vec![CopyBookkeepingVersion {
        corpus: "fleet-sync-receiver".into(),
        stored,
        supported,
    }])
}
async fn read_failures<Scope: TransactionScope>(
    scope: &mut Scope,
) -> Result<Vec<CopyMigrationFailureRecord>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Scope::Database>
        + sqlx_core::encode::Encode<'blob, Scope::Database>
        + sqlx_core::decode::Decode<'blob, Scope::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Scope::Database>
        + sqlx_core::encode::Encode<'scalar, Scope::Database>
        + sqlx_core::decode::Decode<'scalar, Scope::Database>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Scope::Database> + sqlx_core::decode::Decode<'blob, Scope::Database>,
    for<'scalar> Option<i64>: sqlx_core::types::Type<Scope::Database>
        + sqlx_core::decode::Decode<'scalar, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    use crate::fleet_record_codec::required;
    if !table_present(scope, "central_schema_migration_failures").await? {
        return Ok(Vec::new());
    }
    let mut failures = Vec::new();
    for row in crate::generated_bootstrap::read_migration_failures(scope)
        .await
        .map_err(StoreError::backend)?
    {
        let failed_at = crate::migration_bookkeeping::failure_time(&row)?;
        crate::fleet_record_codec::check_private_id(
            &required(row.row_id, "failure row identity")?,
            "failure row identity",
        )?;
        let corpus = required(row.corpus, "failure corpus")?;
        if corpus.is_empty() {
            return Err(StoreError::backend("empty migration failure corpus"));
        }
        let version = u64::try_from(
            row.version
                .ok_or_else(|| StoreError::backend("missing failure version"))?,
        )
        .map_err(StoreError::backend)?;
        failures.push(CopyMigrationFailureRecord {
            corpus,
            version,
            reason: required(row.reason, "failure reason")?,
            at: time(failed_at)?,
        });
    }
    Ok(failures)
}
