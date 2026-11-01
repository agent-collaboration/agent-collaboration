//! Pending entries capture executable work before later definition faults can lose it.
use crate::{
    automation_catalog_record::{AutomationRecord, StoredAutomationDefinition},
    automation_content_store as contents,
    automation_store_error::{AutomationStoreError, AutomationStoreResult, row_id},
    generated_automation_instruction as instructions,
};
use collaboration_protocol::{ContentId, FiringEntry, FiringId, FiringWork, GridTime};
use durable_steps::storage::TransactionScope;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn capture<Scope: TransactionScope>(
    scope: &mut Scope,
    record: &AutomationRecord,
    firing_id: FiringId,
    due_at: GridTime,
) -> AutomationStoreResult<FiringEntry>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let (content, work) = match &record.definition {
        StoredAutomationDefinition::Schedule(schedule) => {
            let row = instructions::read_instruction_document(
                scope,
                schedule.definition.instruction_id.as_str().into(),
            )
            .await?
            .ok_or(AutomationStoreError::InvalidRecord("instruction_id"))?;
            let id: ContentId = crate::automation_store_error::required(
                row.current_content_id,
                "current_content_id",
            )?
            .try_into()
            .map_err(|_| AutomationStoreError::InvalidRecord("current_content_id"))?;
            let content = contents::read(scope, &id)
                .await?
                .ok_or(AutomationStoreError::InvalidRecord("current_content_id"))?;
            if content.owner_id
                != row_id(
                    schedule.definition.instruction_id.as_str(),
                    "instruction_id",
                )?
            {
                return Err(AutomationStoreError::InvalidRecord("content.owner_id"));
            }
            (
                content.reference,
                FiringWork::Schedule {
                    definition: schedule.definition.clone(),
                },
            )
        }
        StoredAutomationDefinition::Wake(wake) => {
            let content = contents::read_wake(scope, &wake.content.content_id)
                .await?
                .ok_or(AutomationStoreError::InvalidRecord("wake.content"))?;
            if content.owner_id != row_id(record.id.as_str(), "automation_id")?
                || content.reference != wake.content
            {
                return Err(AutomationStoreError::InvalidRecord("wake.content"));
            }
            (
                content.reference,
                FiringWork::Wake {
                    created_by: record.creator.declared.clone(),
                    timing: wake.timing_request.clone(),
                    anchor_at: record.header.times().anchor_at.clone(),
                    created_at: record.header.times().created_at.clone(),
                    target: wake.target.clone(),
                    origin: wake.origin.clone(),
                    delivery: wake.delivery,
                    generation_guard: wake.generation_guard.clone(),
                    expires_at: record.wake_expiry()?.clone(),
                },
            )
        }
    };
    Ok(FiringEntry {
        firing_id,
        automation_id: record.id.clone(),
        due_at,
        revision: record.revision.clone(),
        content,
        work,
    })
}

pub(crate) fn destination(entry: &FiringEntry) -> AutomationStoreResult<String> {
    use collaboration_protocol::ExecutionDestination;
    let service = match &entry.work {
        FiringWork::Wake { target, .. } => &target.endpoint.service_id,
        FiringWork::Schedule { definition } => match &definition.destination {
            ExecutionDestination::OwnedThread { target, .. } => &target.endpoint.service_id,
            ExecutionDestination::FreshEachRun { endpoint, .. } => &endpoint.service_id,
            ExecutionDestination::Unprepared | ExecutionDestination::FreshEachRunUnprepared => {
                return Err(AutomationStoreError::InvalidRecord("destination"));
            }
        },
    };
    Ok(String::from(service.clone()))
}
