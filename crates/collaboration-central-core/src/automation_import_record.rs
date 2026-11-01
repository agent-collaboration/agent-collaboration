//! Handover progress is preserved; central never evaluates the legacy local generator.
use crate::{
    automation_catalog_record::{
        AutomationProgress, AutomationRecord, BoundAutomationCreator, CatalogState,
        StoredAutomationDefinition, StoredScheduleDefinition, StoredSuspendedConfiguration,
        StoredSuspendedDefinition, StoredSuspendedWake, StoredWakeDefinition,
    },
    automation_record_header::{AutomationRecordHeader, RecordTimestamps, ScheduleExecutionMode},
    automation_store_error::{AutomationStoreError, AutomationStoreResult, observation_millis},
};
use collaboration_protocol::{
    AutomationImportDefinition, ContentReference, FrozenAutomationImportPayload, GridTime,
    SuspendedAutomationConfiguration, WakeState,
};
use message_board::ServiceId;

pub(crate) fn prepare(
    frozen: &FrozenAutomationImportPayload,
    installation: &ServiceId,
    content: &ContentReference,
    now_ms: i64,
) -> AutomationStoreResult<AutomationRecord> {
    let payload = frozen.payload();
    let timing = || {
        frozen
            .resolved_timing()
            .cloned()
            .ok_or(AutomationStoreError::InvalidRecord(
                "import.resolved_timing",
            ))
    };
    let times = RecordTimestamps {
        anchor_at: payload.header.anchor_at.clone(),
        created_at: payload.header.created_at.clone(),
        updated_at: payload.header.updated_at.clone(),
    };
    let schedule_header = |destination: &collaboration_protocol::ExecutionDestination| {
        AutomationRecordHeader::Schedule {
            times: times.clone(),
            imported_continuity: payload.header.imported_continuity.clone(),
            execution_mode: ScheduleExecutionMode::for_destination(destination),
        }
    };
    let (definition, mut state, header, expires_at) = match &payload.definition {
        AutomationImportDefinition::Schedule(definition) => {
            crate::automation_schedule_validation::retained_definition(definition)?;
            (
                StoredAutomationDefinition::Schedule(StoredScheduleDefinition {
                    definition: definition.clone(),
                    timing: timing()?,
                }),
                if definition.enabled {
                    CatalogState::Enabled
                } else {
                    CatalogState::Disabled
                },
                schedule_header(&definition.destination),
                None,
            )
        }
        AutomationImportDefinition::Wake(definition) => {
            let configuration = &definition.configuration;
            let state = wake_state(configuration.state);
            (
                StoredAutomationDefinition::Wake(StoredWakeDefinition {
                    target: configuration.target.clone(),
                    origin: configuration.origin.clone(),
                    delivery: configuration.delivery,
                    generation_guard: configuration.generation_guard.clone(),
                    content: content.clone(),
                    timing_request: definition.timing.clone(),
                    timing: timing()?,
                }),
                state,
                AutomationRecordHeader::Wake { times },
                Some(configuration.expires_at.clone()),
            )
        }
        AutomationImportDefinition::Suspended(definition) => {
            let (configuration, header, expires, state) = match &definition.configuration {
                SuspendedAutomationConfiguration::Schedule(configuration) => {
                    crate::automation_schedule_validation::retained_configuration(
                        &configuration.destination,
                        configuration.model.as_deref(),
                        configuration.effort.as_deref(),
                        configuration.enabled,
                    )?;
                    (
                        StoredSuspendedConfiguration::Schedule(configuration.clone()),
                        schedule_header(&configuration.destination),
                        None,
                        CatalogState::Suspended,
                    )
                }
                SuspendedAutomationConfiguration::Wake(configuration) => (
                    StoredSuspendedConfiguration::Wake(StoredSuspendedWake {
                        target: configuration.target.clone(),
                        origin: configuration.origin.clone(),
                        delivery: configuration.delivery,
                        generation_guard: configuration.generation_guard.clone(),
                        content: content.clone(),
                        prior_state: configuration.state,
                    }),
                    AutomationRecordHeader::Wake { times },
                    Some(configuration.expires_at.clone()),
                    match configuration.state {
                        WakeState::Active | WakeState::Paused => CatalogState::Suspended,
                        state => wake_state(state),
                    },
                ),
            };
            (
                StoredAutomationDefinition::Suspended(StoredSuspendedDefinition {
                    legacy_timing: definition.legacy_timing.clone(),
                    configuration,
                }),
                state,
                header,
                expires,
            )
        }
    };
    let expiry = expires_at.as_ref().map(observation_millis).transpose()?;
    if matches!(
        state,
        CatalogState::Active | CatalogState::Paused | CatalogState::Suspended
    ) && expiry.is_some_and(|expiry| expiry <= now_ms)
    {
        state = CatalogState::Expired;
    }
    let next_due = payload.first_due_ms.filter(|due| {
        matches!(state, CatalogState::Enabled | CatalogState::Active)
            && expiry.is_none_or(|expiry| due.unix_millis() < expiry)
    });
    Ok(AutomationRecord {
        id: payload.automation_id.clone(),
        revision: payload.revision.clone(),
        definition,
        state,
        creator: BoundAutomationCreator {
            installation: installation.clone(),
            declared: payload.creator.clone(),
        },
        next_due,
        header,
        expires_at,
        progress: AutomationProgress {
            admitted_through: payload.admitted_through_ms,
            external: payload.external_occupant.clone(),
            ..Default::default()
        },
    })
}

fn wake_state(state: WakeState) -> CatalogState {
    match state {
        WakeState::Active => CatalogState::Active,
        WakeState::Paused => CatalogState::Paused,
        WakeState::Cancelled => CatalogState::Cancelled,
        WakeState::Expired => CatalogState::Expired,
        WakeState::Finished => CatalogState::Finished,
    }
}

pub(crate) fn one_shot(
    due: GridTime,
) -> AutomationStoreResult<collaboration_protocol::TimingRequest> {
    Ok(collaboration_protocol::TimingRequest::At {
        at: crate::automation_store_error::timestamp(due.unix_millis())?,
    })
}
