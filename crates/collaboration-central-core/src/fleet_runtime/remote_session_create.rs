//! Session-create admission commits its retained request and immediate Batch together.
use crate::{
    automation_batch_freeze, automation_capability_store,
    automation_store_error::{AutomationStoreError, AutomationStoreResult},
    fleet_row_ids::FleetRowIds,
    machine_work_item_record::{self as records, MachineWorkItemRecord},
    machine_work_item_store as items,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationBatch, AutomationRefusal, BatchId, MachineWorkCustodyState,
    OperationId, RemoteSessionCreateRequest, SessionCreateEntry, WorkEntry, WorkItemId,
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(super) async fn admit_session_create<Store: AtomicStore, Driver: Database>(
    scope: &mut Store::Scope<'_>,
    source: &ServiceId,
    request: &RemoteSessionCreateRequest,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<AutomationAnswer>
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    let digest = records::request_digest(source, request)?;
    if let Some(item) = items::by_operation(scope, &request.operation_id).await? {
        let retained = RemoteSessionCreateRequest {
            operation_id: item.entry.operation_id.clone(),
            definition: item.entry.request.clone(),
        };
        if digest != records::request_digest(&item.entry.source_installation, &retained)? {
            return Ok(AutomationAnswer::Refused(
                AutomationRefusal::OperationConflict {
                    operation_id: request.operation_id.clone(),
                },
            ));
        }
        return Ok(AutomationAnswer::SessionCreate(item.snapshot()));
    }
    records::require_creator(source, &request.definition.created_by)?;
    if !automation_capability_store::eligible(scope, &request.definition.endpoint.service_id)
        .await?
    {
        return Ok(AutomationAnswer::Refused(
            AutomationRefusal::TargetNeedsUpgrade {
                service_id: request.definition.endpoint.service_id.clone(),
            },
        ));
    }
    let work_item_id: WorkItemId = row_ids
        .next()?
        .to_string()
        .try_into()
        .map_err(StoreError::backend)?;
    let batch_id: BatchId = row_ids
        .next()?
        .to_string()
        .try_into()
        .map_err(StoreError::backend)?;
    let entry = SessionCreateEntry {
        work_item_id,
        operation_id: request.operation_id.clone(),
        source_installation: source.clone(),
        request: request.definition.clone(),
    };
    let batch = AutomationBatch {
        batch_id,
        service_id: request.definition.endpoint.service_id.clone(),
        entries: vec![WorkEntry::SessionCreate(entry.clone())]
            .try_into()
            .map_err(StoreError::backend)?,
    };
    items::insert(scope, &entry).await?;
    automation_batch_freeze::publish_batch::<Store, Driver>(scope, &batch, now_ms).await?;
    let item = MachineWorkItemRecord {
        entry,
        target: String::from(request.definition.endpoint.service_id.clone())
            .try_into()
            .map_err(StoreError::backend)?,
        state: MachineWorkCustodyState::Pending,
        result: None,
    };
    Ok(AutomationAnswer::SessionCreate(item.snapshot()))
}

pub(super) async fn read_session_create_status<Scope: TransactionScope>(
    scope: &mut Scope,
    operation_id: &OperationId,
) -> AutomationStoreResult<AutomationAnswer>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let item = items::by_operation(scope, operation_id)
        .await?
        .ok_or(AutomationStoreError::Refused(AutomationRefusal::NotFound))?;
    Ok(AutomationAnswer::SessionCreate(item.snapshot()))
}
