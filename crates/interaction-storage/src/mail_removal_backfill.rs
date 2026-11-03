//! The physical store remembers its completed custody-removal admission pass.
use crate::StorageError;
use durable_steps::{Host, storage::TransactionScope, turso_store::TursoScope};

const MIGRATION_NAME: &str = "mail-record-removal-v1";

struct BackfillRow {
    id: String,
    admitted_count: i64,
    created_at: i64,
    updated_at: i64,
}

pub async fn mail_removal_backfill_completed_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
) -> Result<bool, StorageError> {
    let row = sqlx_turso::query_as!(BackfillRow,
        "SELECT id AS \"id!: String\", admitted_count AS \"admitted_count!: i64\", created_at AS \"created_at!: i64\", updated_at AS \"updated_at!: i64\" FROM mail_removal_backfill WHERE migration_name=?",
        MIGRATION_NAME).fetch_optional(scope.executor()).await?;
    let Some(row) = row else {
        return Ok(false);
    };
    if !uuid::Uuid::parse_str(&row.id).is_ok_and(|id| id.get_version_num() == 7)
        || row.admitted_count < 0
        || row.created_at < 0
        || row.updated_at != row.created_at
    {
        return Err(StorageError::InvalidStoredRecord);
    }
    Ok(true)
}

/// Commit this marker in the same transaction as the SDK's removal admissions.
pub async fn complete_mail_removal_backfill_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    admitted_count: usize,
    completed_at_ms: i64,
) -> Result<(), StorageError> {
    let count = i64::try_from(admitted_count).map_err(|_| StorageError::InvalidRecord)?;
    if completed_at_ms < 0 {
        return Err(StorageError::InvalidRecord);
    }
    let id = uuid::Uuid::now_v7().to_string();
    sqlx_turso::query!("INSERT INTO mail_removal_backfill(id,migration_name,admitted_count,created_at,updated_at) VALUES(?,?,?,?,?)",
        id, MIGRATION_NAME, count, completed_at_ms, completed_at_ms).execute(scope.executor()).await?;
    Ok(())
}
