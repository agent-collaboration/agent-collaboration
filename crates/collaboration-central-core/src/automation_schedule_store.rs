//! Schedule mutations preserve immutable past work and replace future configuration.
use crate::{
    automation_capability_store as capabilities,
    automation_catalog_record::{
        AutomationRecord, BoundAutomationCreator, CatalogState, StoredAutomationDefinition,
        StoredScheduleDefinition,
    },
    automation_catalog_store as catalog, automation_instruction_store as instructions,
    automation_schedule_validation as validation,
    automation_store_error::{AutomationStoreError, AutomationStoreResult, timestamp},
    fleet_row_ids::FleetRowIds,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationId, AutomationKind, AutomationRefusal, AutomationTiming,
    CatalogAutomation, ExecutionDestination, ImportedContinuity, ScheduleCreateRequest,
    ScheduleDefinition, ScheduleId, ScheduleSnapshot, ScheduleUpdateRequest, WakeCreator,
};
use durable_steps::storage::TransactionScope;
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) enum ScheduleEdit<'request> {
    Replace(&'request ScheduleUpdateRequest),
    SetEnabled(bool),
    Cancel,
}

pub(crate) async fn create<Scope: TransactionScope>(
    scope: &mut Scope,
    request: &ScheduleCreateRequest,
    installation: ServiceId,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<AutomationAnswer>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    validation::definition(&request.definition)?;
    let timing =
        AutomationTiming::resolve(&request.definition.timing, AutomationKind::Schedule, now_ms)?;
    let now = timestamp(now_ms)?;
    if instructions::read(scope, &request.definition.instruction_id)
        .await?
        .is_none()
    {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    }
    require_target(scope, &request.definition).await?;
    let id: AutomationId = row_ids
        .next()?
        .to_string()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("automation_id"))?;
    let revision = row_ids
        .next()?
        .to_string()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("revision"))?;
    let record = AutomationRecord {
        id,
        revision,
        state: if request.definition.enabled {
            CatalogState::Enabled
        } else {
            CatalogState::Disabled
        },
        creator: BoundAutomationCreator {
            installation,
            declared: WakeCreator::Router {},
        },
        next_due: request.definition.enabled.then(|| timing.first_due()),
        definition: StoredAutomationDefinition::Schedule(StoredScheduleDefinition {
            definition: request.definition.clone(),
            timing,
            imported_continuity: ImportedContinuity::None,
            anchor_at: now.clone(),
            created_at: now.clone(),
            updated_at: now,
        }),
    };
    let answer = AutomationAnswer::Schedule(snapshot(&record)?);
    catalog::insert(scope, &record).await?;
    Ok(answer)
}

pub(crate) async fn show<Scope: TransactionScope>(
    scope: &mut Scope,
    id: &ScheduleId,
) -> AutomationStoreResult<AutomationAnswer>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let id: AutomationId = id
        .as_str()
        .to_owned()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("automation_id"))?;
    let Some(record) = catalog::read(scope, &id).await? else {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    };
    if !matches!(record.definition, StoredAutomationDefinition::Schedule(_)) {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    }
    Ok(AutomationAnswer::Schedule(snapshot(&record)?))
}

pub(crate) async fn change<Scope: TransactionScope>(
    scope: &mut Scope,
    id: &ScheduleId,
    edit: ScheduleEdit<'_>,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<AutomationAnswer>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let id: AutomationId = id
        .as_str()
        .to_owned()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("automation_id"))?;
    let Some(mut record) = catalog::read(scope, &id).await? else {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    };
    if record.state == CatalogState::Deleted {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    }
    let StoredAutomationDefinition::Schedule(schedule) = &mut record.definition else {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    };
    let before = schedule.definition.clone();
    match edit {
        ScheduleEdit::Replace(request) => {
            if record.revision != request.expected_change_id {
                return Ok(AutomationAnswer::Refused(
                    AutomationRefusal::ScheduleChangeConflict {
                        current_change_id: record.revision,
                    },
                ));
            }
            validation::unchanged_mode(&before.destination, &request.definition.destination)?;
            validation::definition(&request.definition)?;
            require_target(scope, &request.definition).await?;
            if instructions::read(scope, &request.definition.instruction_id)
                .await?
                .is_none()
            {
                return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
            }
            if serde_json::to_value(&before.timing)
                .map_err(|_| AutomationStoreError::InvalidRecord("timing"))?
                != serde_json::to_value(&request.definition.timing)
                    .map_err(|_| AutomationStoreError::InvalidRecord("timing"))?
            {
                schedule.timing = AutomationTiming::resolve(
                    &request.definition.timing,
                    AutomationKind::Schedule,
                    now_ms,
                )?;
            }
            schedule.definition = request.definition.clone();
        }
        ScheduleEdit::SetEnabled(enabled) => {
            schedule.definition.enabled = enabled;
            validation::definition(&schedule.definition)?;
            if enabled {
                require_target(scope, &schedule.definition).await?;
            }
        }
        ScheduleEdit::Cancel => {
            schedule.definition.enabled = false;
            record.state = CatalogState::Deleted;
        }
    }
    schedule.updated_at = timestamp(now_ms)?;
    if record.state != CatalogState::Deleted {
        record.state = if schedule.definition.enabled {
            CatalogState::Enabled
        } else {
            CatalogState::Disabled
        };
    }
    let changed_timing = serde_json::to_value(&before.timing)
        .map_err(|_| AutomationStoreError::InvalidRecord("timing"))?
        != serde_json::to_value(&schedule.definition.timing)
            .map_err(|_| AutomationStoreError::InvalidRecord("timing"))?;
    if changed_timing || before.enabled != schedule.definition.enabled {
        record.next_due = if schedule.definition.enabled {
            schedule.timing.next_after(now_ms)?
        } else {
            None
        };
    }
    if record.state == CatalogState::Deleted {
        record.next_due = None;
    }
    record.revision = row_ids
        .next()?
        .to_string()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("revision"))?;
    let already_sent = crate::automation_firing_store::sent_firings(scope, &record.id).await?;
    let answer = AutomationAnswer::Mutation {
        automation: CatalogAutomation::Schedule(snapshot(&record)?),
        already_sent,
    };
    catalog::update(scope, &record).await?;
    Ok(answer)
}

async fn require_target<Scope: TransactionScope>(
    scope: &mut Scope,
    definition: &ScheduleDefinition,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let service = match &definition.destination {
        ExecutionDestination::Unprepared | ExecutionDestination::FreshEachRunUnprepared => {
            return Ok(());
        }
        ExecutionDestination::OwnedThread { target, .. } => &target.endpoint.service_id,
        ExecutionDestination::FreshEachRun { endpoint, .. } => &endpoint.service_id,
    };
    if !capabilities::eligible(scope, service).await? {
        return Err(AutomationStoreError::Refused(
            AutomationRefusal::TargetNeedsUpgrade {
                service_id: service.clone(),
            },
        ));
    }
    Ok(())
}

pub(crate) fn snapshot(record: &AutomationRecord) -> AutomationStoreResult<ScheduleSnapshot> {
    let StoredAutomationDefinition::Schedule(schedule) = &record.definition else {
        return Err(AutomationStoreError::InvalidRecord("schedule.kind"));
    };
    Ok(ScheduleSnapshot {
        schedule_id: record
            .id
            .as_str()
            .to_owned()
            .try_into()
            .map_err(|_| AutomationStoreError::InvalidRecord("schedule_id"))?,
        change_id: record.revision.clone(),
        definition: schedule.definition.clone(),
        imported_continuity: schedule.imported_continuity.clone(),
        anchor_at: schedule.anchor_at.clone(),
        created_at: schedule.created_at.clone(),
        updated_at: schedule.updated_at.clone(),
        next_due_at: record
            .next_due
            .map(|due| timestamp(due.unix_millis()))
            .transpose()?,
        active_run_id: None,
        waiting_run_id: None,
    })
}
