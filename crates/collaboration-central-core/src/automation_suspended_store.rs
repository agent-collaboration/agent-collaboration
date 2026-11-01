//! Suspended definitions retain configuration and legacy text without a clock.
use crate::{
    automation_catalog_record::{
        AutomationRecord, CatalogState, StoredAutomationDefinition, StoredSuspendedConfiguration,
        StoredSuspendedDefinition, wake_slot,
    },
    automation_record_header::AutomationRecordHeader,
    automation_store_error::{AutomationStoreError, AutomationStoreResult},
};
use collaboration_protocol::{
    AutomationImportHeader, AutomationKind, GridTime, ImportedWakeConfiguration,
    ObservationTimestamp, SuspendedAutomationSnapshot, SuspendedAutomationSnapshotConfiguration,
    WakeState,
};

pub(crate) fn validate(
    definition: &StoredSuspendedDefinition,
    kind: AutomationKind,
    state: CatalogState,
    header: &AutomationRecordHeader,
    next_due: Option<GridTime>,
    expires: Option<&ObservationTimestamp>,
    slot: Option<&str>,
) -> AutomationStoreResult<()> {
    let invalid = || AutomationStoreError::InvalidRecord("suspended.configuration");
    if next_due.is_some() {
        return Err(AutomationStoreError::InvalidRecord("next_due_ms"));
    }
    match &definition.configuration {
        StoredSuspendedConfiguration::Schedule(configuration) => {
            if kind != AutomationKind::Schedule
                || state != CatalogState::Suspended
                || expires.is_some()
                || slot.is_some()
            {
                return Err(invalid());
            }
            let AutomationRecordHeader::Schedule { execution_mode, .. } = header else {
                return Err(invalid());
            };
            execution_mode.require_destination(&configuration.destination)?;
            crate::automation_schedule_validation::retained_configuration(
                &configuration.destination,
                configuration.model.as_deref(),
                configuration.effort.as_deref(),
                configuration.enabled,
            )?;
        }
        StoredSuspendedConfiguration::Wake(configuration) => {
            if kind != AutomationKind::Wake
                || !matches!(
                    state,
                    CatalogState::Suspended
                        | CatalogState::Expired
                        | CatalogState::Cancelled
                        | CatalogState::Finished
                        | CatalogState::Replaced
                )
                || expires.is_none()
                || !matches!(header, AutomationRecordHeader::Wake { .. })
                || (state == CatalogState::Suspended
                    && !matches!(
                        configuration.prior_state,
                        WakeState::Active | WakeState::Paused
                    ))
            {
                return Err(invalid());
            }
            let expected = if state.owns_wake_slot() {
                Some(wake_slot(&configuration.target)?)
            } else {
                None
            };
            if slot != expected.as_deref() {
                return Err(AutomationStoreError::InvalidRecord("slot_session_key"));
            }
        }
    }
    Ok(())
}

pub(crate) fn snapshot(
    record: &AutomationRecord,
) -> AutomationStoreResult<SuspendedAutomationSnapshot> {
    let StoredAutomationDefinition::Suspended(definition) = &record.definition else {
        return Err(AutomationStoreError::InvalidRecord("suspended.kind"));
    };
    let configuration = match &definition.configuration {
        StoredSuspendedConfiguration::Schedule(configuration) => {
            SuspendedAutomationSnapshotConfiguration::Schedule(configuration.clone())
        }
        StoredSuspendedConfiguration::Wake(configuration) => {
            let state = match record.state {
                CatalogState::Suspended => configuration.prior_state,
                CatalogState::Expired => WakeState::Expired,
                CatalogState::Cancelled => WakeState::Cancelled,
                CatalogState::Finished => WakeState::Finished,
                CatalogState::Replaced => WakeState::Cancelled,
                _ => return Err(AutomationStoreError::InvalidRecord("suspended.state")),
            };
            SuspendedAutomationSnapshotConfiguration::Wake {
                configuration: ImportedWakeConfiguration {
                    target: configuration.target.clone(),
                    origin: configuration.origin.clone(),
                    delivery: configuration.delivery,
                    generation_guard: configuration.generation_guard.clone(),
                    expires_at: record.wake_expiry()?.clone(),
                    state,
                },
                created_by: record.creator.declared.clone(),
                content: configuration.content.clone(),
            }
        }
    };
    let times = record.header.times();
    let imported_continuity = match &record.header {
        AutomationRecordHeader::Schedule {
            imported_continuity,
            ..
        } => imported_continuity.clone(),
        AutomationRecordHeader::Wake { .. } => None,
    };
    Ok(SuspendedAutomationSnapshot {
        automation_id: record.id.clone(),
        kind: record.definition.kind(),
        revision: record.revision.clone(),
        legacy_timing: definition.legacy_timing.clone(),
        configuration,
        header: AutomationImportHeader {
            anchor_at: times.anchor_at.clone(),
            created_at: times.created_at.clone(),
            updated_at: times.updated_at.clone(),
            imported_continuity,
        },
    })
}
