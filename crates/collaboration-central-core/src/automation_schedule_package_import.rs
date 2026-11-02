//! Portable Schedule and Instruction identities enter through one Fleet transaction.
use crate::{
    automation_catalog_record::{
        AutomationRecord, BoundAutomationCreator, CatalogState, StoredAutomationDefinition,
        StoredScheduleDefinition,
    },
    automation_catalog_store as catalog, automation_instruction_store as instructions,
    automation_record_header::{AutomationRecordHeader, RecordTimestamps, ScheduleExecutionMode},
    automation_schedule_validation,
    automation_store_error::{AutomationStoreError, AutomationStoreResult, timestamp},
    fleet_row_ids::FleetRowIds,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationId, AutomationKind, AutomationRefusal, AutomationRequestConstraint,
    AutomationTiming, CatalogAutomation, ExecutionDestination, ImportedContinuity,
    SchedulePackageImportRequest, WakeCreator,
};
use durable_steps::storage::TransactionScope;
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn import_package<Scope: TransactionScope>(
    scope: &mut Scope,
    request: &SchedulePackageImportRequest,
    installation: &ServiceId,
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
    let package = &request.package;
    if package.definition.instruction_id != package.instruction.instruction_id {
        return Err(AutomationStoreError::InvalidRequest {
            field: "package.definition.instructionId",
            reason: AutomationRequestConstraint::MalformedShape,
        });
    }
    if !matches!(
        package.definition.destination,
        ExecutionDestination::Unprepared | ExecutionDestination::FreshEachRunUnprepared
    ) {
        return Err(AutomationStoreError::InvalidRequest {
            field: "package.definition.destination",
            reason: AutomationRequestConstraint::UnpreparedDestination,
        });
    }
    let mut definition = package.definition.clone();
    definition.enabled = false;
    automation_schedule_validation::definition(&definition)?;
    let timing = AutomationTiming::resolve(&definition.timing, AutomationKind::Schedule, now_ms)?;
    let id: AutomationId = package
        .schedule_id
        .as_str()
        .to_owned()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("automation_id"))?;
    let previous = catalog::read(scope, &id).await?;
    if let Some(previous) = &previous {
        if !matches!(previous.definition, StoredAutomationDefinition::Schedule(_)) {
            return Err(AutomationStoreError::Refused(AutomationRefusal::NotFound));
        }
        if !request.overwrite {
            return Err(AutomationStoreError::Refused(
                AutomationRefusal::ScheduleImportExists {
                    schedule_id: package.schedule_id.clone(),
                },
            ));
        }
        let AutomationRecordHeader::Schedule { execution_mode, .. } = &previous.header else {
            return Err(AutomationStoreError::InvalidRecord(
                "schedule.record_header",
            ));
        };
        execution_mode.require_destination(&definition.destination)?;
    }
    let instruction = instructions::read(scope, &package.instruction.instruction_id).await?;
    if let Some(instruction) = &instruction
        && instruction.text != package.instruction.text
    {
        return Err(AutomationStoreError::Refused(
            AutomationRefusal::InstructionImportConflict {
                instruction_id: package.instruction.instruction_id.clone(),
                schedule_ids: catalog::referencing_schedules(
                    scope,
                    &package.instruction.instruction_id,
                )
                .await?,
            },
        ));
    }
    let now = timestamp(now_ms)?;
    let created_at = previous
        .as_ref()
        .map(|record| record.header.times().created_at.clone())
        .unwrap_or_else(|| now.clone());
    let header = AutomationRecordHeader::Schedule {
        times: RecordTimestamps {
            anchor_at: now.clone(),
            created_at,
            updated_at: now,
        },
        imported_continuity: Some(match &package.continuity {
            Some(summary) => ImportedContinuity::ImportedSummary {
                text: summary.text.as_str().into(),
                source_run_id: String::from(summary.source_run_id.clone()),
                source_target: summary.source_target.clone(),
                import_operation_id: request.operation_id.clone(),
            },
            None => ImportedContinuity::None,
        }),
        execution_mode: ScheduleExecutionMode::for_destination(&definition.destination),
    };
    let existed = previous.is_some();
    let revision = row_ids
        .next()?
        .to_string()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("revision"))?;
    let (creator, progress) = previous
        .map(|record| (record.creator, record.progress))
        .unwrap_or_else(|| {
            (
                BoundAutomationCreator {
                    installation: installation.clone(),
                    declared: WakeCreator::Router {},
                },
                Default::default(),
            )
        });
    let record = AutomationRecord {
        id,
        revision,
        definition: StoredAutomationDefinition::Schedule(StoredScheduleDefinition {
            definition,
            timing,
        }),
        state: CatalogState::Disabled,
        creator,
        next_due: None,
        header,
        expires_at: None,
        progress,
    };
    let pending = crate::automation_pending_mutation::prepare(scope, &record).await?;
    let answer = AutomationAnswer::Mutation {
        automation: CatalogAutomation::Schedule(crate::automation_schedule_store::snapshot(
            &record,
        )?),
        already_sent: crate::automation_firing_store::sent_firings(scope, &record.id).await?,
    };
    // Every expected refusal above precedes writes to either catalog owner.
    if instruction.is_none() {
        instructions::insert_with_id(
            scope,
            &package.instruction.instruction_id,
            &package.instruction.text,
            now_ms,
            row_ids,
        )
        .await?;
    }
    if existed {
        catalog::update(scope, &record).await?;
        catalog::update_imported_schedule_header(scope, &record).await?;
    } else {
        catalog::insert(scope, &record).await?;
    }
    crate::automation_pending_mutation::commit(scope, pending).await?;
    Ok(answer)
}
