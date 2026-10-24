//! A typed rollback marker carries the migration context through the shared store.
use durable_steps::{OperationFailure, StoreError, storage::TransactionScope};
use runtime_primitives::SyncTime;
use serde::{Deserialize, Serialize};
use sqlx_core::{arguments::IntoArguments, database::Database, encode::Encode, types::Type};

const ROLLBACK_CODE: &str = "central_schema_migration";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MigrationFailure {
    pub corpus: String,
    pub version: u64,
    pub reason: String,
    pub at: SyncTime,
    pub cause: StoreError,
}
impl MigrationFailure {
    pub(crate) fn rollback(
        corpus: &str,
        version: i64,
        at: SyncTime,
        cause: StoreError,
    ) -> StoreError {
        let Ok(version) = u64::try_from(version) else {
            return cause;
        };
        let reason = match fleet_sync_receiver::classify_copy_store_error(cause.clone()) {
            fleet_sync_receiver::CopyTransactionFailure::Unavailable(
                fleet_sync_protocol::CopyUnavailableReason::Damaged { detail },
            ) => detail,
            _ => cause.to_string(),
        };
        let failure = Self {
            corpus: corpus.into(),
            version,
            reason,
            at,
            cause,
        };
        match serde_json::to_string(&failure) {
            Ok(detail) => StoreError::StepRollback {
                failure: Box::new(OperationFailure::Refused {
                    code: ROLLBACK_CODE.into(),
                    detail,
                }),
            },
            Err(_) => failure.cause,
        }
    }
    pub(crate) fn from_rollback(error: &StoreError) -> Option<Self> {
        let StoreError::StepRollback { failure } = error else {
            return None;
        };
        let OperationFailure::Refused { code, detail } = failure.as_ref() else {
            return None;
        };
        if code != ROLLBACK_CODE {
            return None;
        }
        let failure: Self = serde_json::from_str(detail).ok()?;
        if failure.corpus.is_empty() || failure.version == 0 {
            return None;
        }
        Some(failure)
    }
    pub(crate) fn unavailable(&self) -> fleet_sync_protocol::CopyUnavailableReason {
        match fleet_sync_receiver::classify_copy_store_error(self.cause.clone()) {
            fleet_sync_receiver::CopyTransactionFailure::Unavailable(
                fleet_sync_protocol::CopyUnavailableReason::StorageFull,
            ) => fleet_sync_protocol::CopyUnavailableReason::StorageFull,
            _ => fleet_sync_protocol::CopyUnavailableReason::MigrationPending {
                version: self.version,
            },
        }
    }
}
pub(crate) async fn record<Scope: TransactionScope>(
    scope: &mut Scope,
    failure: MigrationFailure,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    crate::generated_bootstrap::initialize_migration_failures(scope)
        .await
        .map_err(StoreError::backend)?;
    crate::generated_bootstrap::record_migration_failure(
        scope,
        durable_steps::Uuid::now_v7().to_string(),
        failure.corpus,
        failure.version.to_string(),
        failure.reason,
        failure.at.unix_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}
