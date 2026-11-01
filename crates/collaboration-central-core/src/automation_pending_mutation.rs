//! Catalog edits replace or cancel only entries still before the Batch cutoff.
use crate::{
    automation_catalog_record::{AutomationRecord, CatalogState},
    automation_entry_capture,
    automation_store_error::{
        AutomationStoreError, AutomationStoreResult, required, timestamp_millis,
    },
    generated_automation_admission as admissions, generated_automation_custody as custody,
};
use collaboration_protocol::{AutomationId, FiringId, GridTime};
use durable_steps::{StoreError, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) enum PendingMutation {
    Cancel(AutomationId),
    Replace(Vec<String>),
}
pub(crate) async fn prepare<Scope: TransactionScope>(
    scope: &mut Scope,
    record: &AutomationRecord,
) -> AutomationStoreResult<PendingMutation>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    if !matches!(record.state, CatalogState::Enabled | CatalogState::Active) {
        return Ok(PendingMutation::Cancel(record.id.clone()));
    }
    let rows = custody::list_pending_entries(scope, record.id.as_str().into()).await?;
    let mut entries = Vec::new();
    for row in rows {
        if row.report_service_id.is_some() {
            return Err(AutomationStoreError::InvalidRecord(
                "pending.report_service_id",
            ));
        }
        let firing_id: FiringId = required(row.firing_id, "firing_id")?
            .try_into()
            .map_err(|_| AutomationStoreError::InvalidRecord("firing_id"))?;
        let due_at = GridTime::new(timestamp_millis(row.due_ms, "due_ms")?)?;
        let entry = automation_entry_capture::capture(scope, record, firing_id, due_at).await?;
        automation_entry_capture::destination(&entry)?;
        entries.push(
            serde_json::to_string(&entry)
                .map_err(|_| AutomationStoreError::InvalidRecord("pending_entry_json"))?,
        );
    }
    Ok(PendingMutation::Replace(entries))
}
pub(crate) async fn commit<Scope: TransactionScope>(
    scope: &mut Scope,
    mutation: PendingMutation,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    match mutation {
        PendingMutation::Cancel(id) => cancel(scope, &id).await?,
        PendingMutation::Replace(entries) => {
            for entry in entries {
                custody::replace_pending_entry(scope, entry)
                    .await
                    .map_err(StoreError::backend)?;
            }
        }
    }
    Ok(())
}
pub(crate) async fn cancel<Scope: TransactionScope>(
    scope: &mut Scope,
    id: &AutomationId,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    admissions::cancel_pending_firings(scope, id.as_str().into())
        .await
        .map_err(StoreError::backend)?;
    custody::clear_cancelled_occupancy(scope, id.as_str().into())
        .await
        .map_err(StoreError::backend)?;
    Ok(())
}
