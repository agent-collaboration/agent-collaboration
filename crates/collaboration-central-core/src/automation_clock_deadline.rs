//! Automation calls and Fleet alarms restore the durable Automation and journal minimum.
use crate::{automation_store_error::timestamp_millis, generated_automation_catalog as queries};
use durable_steps::{
    JournalScope, StoreError,
    storage::{JournalStorage, TransactionScope},
};
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn request_next<Scope: TransactionScope + JournalScope + JournalStorage>(
    scope: &mut Scope,
) -> Result<Option<SyncTime>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let rows = queries::list_automation_deadlines(scope)
        .await
        .map_err(StoreError::backend)?;
    // Evaluation reports a malformed deadline as a permanent row fault. Timer
    // selection rejects that candidate without coercion, so it cannot become
    // an alarm or block the valid deadlines of other Automations.
    let deadline = rows
        .into_iter()
        .filter_map(|row| timestamp_millis(row.deadline_ms, "automation_deadline").ok())
        .map(SyncTime::from_unix_millis)
        .min();
    // Journal debt is already durable. Reading it here includes due notice work
    // without creating a future journal operation for an Automation timer.
    let journal = scope.earliest().await?;
    let earliest = match (deadline, journal) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    };
    if let Some(at) = earliest {
        scope.ensure_wake(at).await?;
    }
    Ok(earliest)
}
