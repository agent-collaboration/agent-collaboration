//! Only acknowledged custody crosses the two-calendar-month retention boundary.
use durable_steps::{StoreError, storage::TransactionScope};
use sqlx_core::{arguments::IntoArguments, database::Database, encode::Encode, types::Type};

pub(crate) async fn prune<Scope: TransactionScope>(
    scope: &mut Scope,
    now_ms: i64,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let cutoff = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now_ms)
        .and_then(|now| now.checked_sub_months(chrono::Months::new(2)))
        .ok_or_else(|| StoreError::backend("invalid Automation retention clock"))?
        .timestamp_millis();
    crate::generated_automation_custody::prune_acknowledged_batches(scope, cutoff.to_string())
        .await
        .map_err(StoreError::backend)?;
    Ok(())
}
