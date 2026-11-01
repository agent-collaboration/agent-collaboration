//! Schedule mutations preserve immutable past work and replace future configuration.
use crate::{
    automation_capability_store as capabilities,
    automation_catalog_record::{
        AutomationRecord, BoundAutomationCreator, CatalogState, StoredAutomationDefinition,
        StoredScheduleDefinition,
    },
    automation_catalog_store as catalog, automation_instruction_store as instructions,
    automation_record_header::AutomationRecordHeader,
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
        header: AutomationRecordHeader::schedule(&request.definition.destination, now_ms)?,
        expires_at: None,
        progress: Default::default(),
        definition: StoredAutomationDefinition::Schedule(StoredScheduleDefinition {
            definition: request.definition.clone(),
            timing,
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
    let Some(row) =
        crate::generated_automation_catalog::read_automation(scope, id.as_str().into()).await?
    else {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    };
    if row.kind.as_deref() != Some("schedule") {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    }
    if let Some(faulted) = crate::automation_fault_projection::projection(&row)? {
        return Ok(AutomationAnswer::Faulted(faulted));
    }
    let record = catalog::decode(row)?;
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
    let Some(row) =
        crate::generated_automation_catalog::read_automation(scope, id.as_str().into()).await?
    else {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    };
    if crate::automation_fault_projection::projection(&row)?.is_some() {
        return match edit {
            ScheduleEdit::Replace(request) => repair(scope, row, request, now_ms, row_ids).await,
            _ => Err(AutomationStoreError::InvalidRecord("faulted_automation")),
        };
    }
    let mut record = catalog::decode(row)?;
    if record.state == CatalogState::Deleted {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    }
    let AutomationRecordHeader::Schedule { execution_mode, .. } = &record.header else {
        return Err(AutomationStoreError::InvalidRecord(
            "schedule.record_header",
        ));
    };
    let execution_mode = *execution_mode;
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
            execution_mode.require_destination(&request.definition.destination)?;
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
    record.header.set_updated(now_ms)?;
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
    let pending = crate::automation_pending_mutation::prepare(scope, &record).await?;
    catalog::update(scope, &record).await?;
    crate::automation_pending_mutation::commit(scope, pending).await?;
    Ok(answer)
}

async fn repair<Scope: TransactionScope>(
    scope: &mut Scope,
    mut row: crate::generated_automation_catalog::StoredAutomation,
    request: &ScheduleUpdateRequest,
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
    let faulted = crate::automation_fault_projection::projection(&row)?
        .ok_or(AutomationStoreError::InvalidRecord("fault_json"))?;
    if faulted.kind != AutomationKind::Schedule {
        return Err(AutomationStoreError::Refused(AutomationRefusal::NotFound));
    }
    if faulted.revision != request.expected_change_id {
        return Err(AutomationStoreError::Refused(
            AutomationRefusal::ScheduleChangeConflict {
                current_change_id: faulted.revision,
            },
        ));
    }
    let AutomationRecordHeader::Schedule { execution_mode, .. } =
        catalog::decode_header(&row, AutomationKind::Schedule)?
    else {
        return Err(AutomationStoreError::InvalidRecord("record_header"));
    };
    execution_mode.require_destination(&request.definition.destination)?;
    validation::definition(&request.definition)?;
    require_target(scope, &request.definition).await?;
    if instructions::read(scope, &request.definition.instruction_id)
        .await?
        .is_none()
    {
        return Err(AutomationStoreError::Refused(AutomationRefusal::NotFound));
    }
    let timing =
        AutomationTiming::resolve(&request.definition.timing, AutomationKind::Schedule, now_ms)?;
    row.next_due_ms = if request.definition.enabled {
        timing
            .next_after(now_ms)?
            .map(|due| due.unix_millis().to_string())
    } else {
        None
    };
    row.definition_json = Some(
        serde_json::to_string(&StoredAutomationDefinition::Schedule(
            StoredScheduleDefinition {
                definition: request.definition.clone(),
                timing,
            },
        ))
        .map_err(|_| AutomationStoreError::InvalidRecord("definition_json"))?,
    );
    row.state = Some(
        if request.definition.enabled {
            "enabled"
        } else {
            "disabled"
        }
        .into(),
    );
    row.revision = Some(row_ids.next()?.to_string());
    row.fault_json = None;
    row.faulted_at_ms = None;
    // The validated replacement changes only executable fields and revision.
    // The original independent header, including updated_at, is retained.
    let record = catalog::decode(row)?;
    let pending = crate::automation_pending_mutation::prepare(scope, &record).await?;
    let answer = AutomationAnswer::Mutation {
        automation: CatalogAutomation::Schedule(snapshot(&record)?),
        already_sent: crate::automation_firing_store::sent_firings(scope, &record.id).await?,
    };
    catalog::update(scope, &record).await?;
    crate::automation_pending_mutation::commit(scope, pending).await?;
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
    let AutomationRecordHeader::Schedule {
        times,
        imported_continuity,
        ..
    } = &record.header
    else {
        return Err(AutomationStoreError::InvalidRecord(
            "schedule.record_header",
        ));
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
        imported_continuity: imported_continuity
            .clone()
            .unwrap_or(ImportedContinuity::None),
        anchor_at: times.anchor_at.clone(),
        created_at: times.created_at.clone(),
        updated_at: times.updated_at.clone(),
        next_due_at: record
            .next_due
            .map(|due| timestamp(due.unix_millis()))
            .transpose()?,
        active_run_id: None,
        waiting_run_id: None,
    })
}
