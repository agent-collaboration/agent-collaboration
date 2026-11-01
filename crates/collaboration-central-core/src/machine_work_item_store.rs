//! Machine-work requests stay immutable while custody and reported facts advance.
use crate::{
    automation_store_error::{AutomationStoreError, AutomationStoreResult},
    generated_machine_work_item as queries,
    machine_work_item_record::{self as records, MachineWorkItemRecord, MachineWorkResult},
};
use collaboration_protocol::{
    AutomationBatch, MachineWorkCustodyState, OperationId, RemoteSessionCreateRequest,
    SessionCreateEntry, WorkEntry, WorkItemId,
};
use durable_steps::storage::TransactionScope;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    id: &WorkItemId,
) -> AutomationStoreResult<Option<MachineWorkItemRecord>>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let record = queries::read_machine_work_item(scope, id.as_str().into())
        .await?.map(records::decode).transpose()?;
    if record.as_ref().is_some_and(|record| record.entry.work_item_id != *id) {
        return Err(AutomationStoreError::InvalidRecord("machine_work_items.work_item_id"));
    }
    Ok(record)
}

pub(crate) async fn by_operation<Scope: TransactionScope>(
    scope: &mut Scope,
    id: &OperationId,
) -> AutomationStoreResult<Option<MachineWorkItemRecord>>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let record = queries::read_machine_work_operation(scope, id.as_str().into())
        .await?.map(records::decode).transpose()?;
    if record.as_ref().is_some_and(|record| record.entry.operation_id != *id) {
        return Err(AutomationStoreError::InvalidRecord("machine_work_items.operation_id"));
    }
    Ok(record)
}

pub(crate) async fn insert<Scope: TransactionScope>(
    scope: &mut Scope,
    entry: &SessionCreateEntry,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    records::require_creator(&entry.source_installation, &entry.request.created_by)?;
    let request = RemoteSessionCreateRequest {
        operation_id: entry.operation_id.clone(),
        definition: entry.request.clone(),
    };
    let digest = records::request_digest(&entry.source_installation, &request)?;
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Admission<'entry> {
        entry: &'entry SessionCreateEntry,
        request_digest: collaboration_mail_protocol::MailDigest,
    }
    let encoded = serde_json::to_string(&Admission { entry, request_digest: digest })
        .map_err(|_| AutomationStoreError::InvalidRecord("machine_work_items.request_json"))?;
    queries::insert_machine_work_item(scope, encoded).await?;
    Ok(())
}

pub(crate) async fn write_result<Scope: TransactionScope>(
    scope: &mut Scope,
    id: &WorkItemId,
    result: &MachineWorkResult,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let encoded = serde_json::to_string(result)
        .map_err(durable_steps::StoreError::backend)?;
    queries::write_machine_work_result(scope, encoded, id.as_str().into()).await?;
    Ok(())
}

pub(crate) async fn validate_batch<Scope: TransactionScope>(
    scope: &mut Scope,
    batch: &AutomationBatch,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    for entry in batch.entries.entries() {
        let WorkEntry::SessionCreate(entry) = entry else { continue };
        let invalid = || AutomationStoreError::InvalidRecord("batch.work_item");
        let stored = read(scope, &entry.work_item_id).await?.ok_or_else(invalid)?;
        if stored.target.as_str() != String::from(batch.service_id.clone())
            || serde_json::to_vec(&stored.entry).map_err(|_| invalid())?
                != serde_json::to_vec(entry).map_err(|_| invalid())?
        {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(crate) async fn advance_batch_custody<Scope: TransactionScope>(
    scope: &mut Scope,
    batch: &AutomationBatch,
    state: MachineWorkCustodyState,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    validate_batch(scope, batch).await?;
    for entry in batch.entries.entries() {
        let WorkEntry::SessionCreate(entry) = entry else { continue };
        match state {
            MachineWorkCustodyState::Held => queries::hold_machine_work_item(scope, entry.work_item_id.as_str().into()).await?,
            MachineWorkCustodyState::Received => queries::receive_machine_work_item(scope, entry.work_item_id.as_str().into()).await?,
            MachineWorkCustodyState::Pending => return Err(AutomationStoreError::InvalidRecord("machine_work_items.state")),
        }
    }
    Ok(())
}
