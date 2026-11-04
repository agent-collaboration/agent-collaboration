//! Refuse corrupt native bookkeeping times at bootstrap and tolerant health reads.
use crate::generated_bootstrap::{StoredMigrationFailure, StoredMigrationHistory};
use durable_steps::StoreError;
use runtime_primitives::SyncTime;

pub(crate) fn validate_history_times(row: &StoredMigrationHistory) -> Result<(), StoreError> {
    integer_millis(
        row.created_at.as_deref(),
        row.created_class.as_deref(),
        "central_schema_migrations.created_at",
    )?;
    integer_millis(
        row.updated_at.as_deref(),
        row.updated_class.as_deref(),
        "central_schema_migrations.updated_at",
    )?;
    integer_millis(
        row.installed_on.as_deref(),
        row.installed_class.as_deref(),
        "central_schema_migrations.installed_on",
    )?;
    for (field, valid) in [
        (
            "version",
            row.version_class.as_deref() == Some("integer") && row.version.is_some(),
        ),
        (
            "description",
            row.description
                .as_ref()
                .is_some_and(|value| !value.is_empty()),
        ),
        (
            "success",
            row.success_class.as_deref() == Some("integer") && row.success == Some(1),
        ),
        (
            "execution_time",
            row.execution_class.as_deref() == Some("integer")
                && row.execution_time.is_some_and(|elapsed| elapsed >= 0),
        ),
    ] {
        if !valid {
            return Err(StoreError::Corrupt {
                field: format!("central_schema_migrations.{field}"),
                message: "invalid migration bookkeeping".into(),
            });
        }
    }
    Ok(())
}

pub(crate) fn failure_time(row: &StoredMigrationFailure) -> Result<SyncTime, StoreError> {
    integer_millis(
        row.created_at.as_deref(),
        row.created_class.as_deref(),
        "central_schema_migration_failures.created_at",
    )?;
    integer_millis(
        row.updated_at.as_deref(),
        row.updated_class.as_deref(),
        "central_schema_migration_failures.updated_at",
    )?;
    Ok(SyncTime::from_unix_millis(integer_millis(
        row.failed_at.as_deref(),
        row.failed_class.as_deref(),
        "central_schema_migration_failures.failed_at",
    )?))
}

fn integer_millis(
    value: Option<&str>,
    class: Option<&str>,
    field: &str,
) -> Result<i64, StoreError> {
    let invalid = || StoreError::Corrupt {
        field: field.into(),
        message: "timestamp must be a canonical SQLite integer".into(),
    };
    let raw = value.ok_or_else(invalid)?;
    let millis = raw.parse::<i64>().map_err(|_| invalid())?;
    if class != Some("integer") || millis.to_string() != raw {
        return Err(invalid());
    }
    Ok(millis)
}
