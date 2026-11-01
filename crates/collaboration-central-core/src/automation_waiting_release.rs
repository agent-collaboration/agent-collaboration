//! Settlement releases the retained pending entry without reading its Automation's body.
use crate::{
    automation_store_error::{
        AutomationStoreError, AutomationStoreResult, required, timestamp_millis,
    },
    generated_automation_custody as queries,
};
use collaboration_protocol::{AutomationId, FiringEntry, FiringId};
use durable_steps::{StoreError, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

fn decode(
    row: queries::StoredPendingEntry,
    id: &FiringId,
    automation: &AutomationId,
) -> AutomationStoreResult<Option<FiringEntry>> {
    if row.report_service_id.is_some() {
        return Err(AutomationStoreError::InvalidRecord(
            "pending.report_service_id",
        ));
    }
    if required(row.firing_id, "firing_id")? != id.as_str()
        || required(row.automation_id, "automation_id")? != automation.as_str()
    {
        return Err(AutomationStoreError::InvalidRecord("pending.identity"));
    }
    if row.state.as_deref() == Some("cancelled")
        && row.pending_entry_json.is_none()
        && row.batch_id.is_none()
    {
        return Ok(None);
    }
    if row.state.as_deref() != Some("pending") || row.batch_id.is_some() {
        return Err(AutomationStoreError::InvalidRecord("pending.state"));
    }
    let raw: serde_json::Value =
        serde_json::from_str(&required(row.pending_entry_json, "pending_entry_json")?)
            .map_err(|_| AutomationStoreError::InvalidRecord("pending_entry_json"))?;
    crate::automation_request_decode::identity_limits(&raw)?;
    crate::automation_request_decode::counter_limits(&raw)?;
    let entry: FiringEntry = serde_json::from_value(raw)
        .map_err(|_| AutomationStoreError::InvalidRecord("pending_entry_json"))?;
    if entry.firing_id != *id
        || entry.automation_id != *automation
        || entry.due_at.unix_millis() != timestamp_millis(row.due_ms, "due_ms")?
        || entry.revision.as_str() != required(row.revision, "revision")?
        || entry.content.content_id.as_str() != required(row.content_id, "content_id")?
    {
        return Err(AutomationStoreError::InvalidRecord("pending.entry"));
    }
    crate::automation_entry_capture::destination(&entry)?;
    Ok(Some(entry))
}

pub(crate) async fn prepare<Scope: TransactionScope>(
    scope: &mut Scope,
    row: queries::StoredCustodyProgress,
    automation: &AutomationId,
) -> AutomationStoreResult<Option<FiringEntry>>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    if required(row.automation_id, "automation_id")? != automation.as_str() {
        return Err(AutomationStoreError::InvalidRecord("automation_id"));
    }
    let Some(waiting) = row.waiting_firing_id else {
        return Ok(None);
    };
    let id: FiringId = waiting
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("waiting_firing_id"))?;
    let row = queries::read_pending_entry(scope, id.as_str().into())
        .await?
        .ok_or(AutomationStoreError::InvalidRecord("waiting_firing_id"))?;
    decode(row, &id, automation)
}

pub(crate) async fn commit<Scope: TransactionScope>(
    scope: &mut Scope,
    automation: &AutomationId,
    entry: Option<&FiringEntry>,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    queries::promote_automation_waiter(
        scope,
        entry.map(|entry| entry.firing_id.as_str().into()),
        automation.as_str().into(),
    )
    .await
    .map_err(StoreError::backend)?;
    Ok(())
}
