//! Slot replacement uses independent provenance even when executable JSON is corrupt.
use crate::{
    automation_catalog_record::{
        AutomationRecord, BoundAutomationCreator, CatalogState, StoredAutomationDefinition,
        timestamp_millis,
    },
    automation_catalog_store as catalog,
    automation_store_error::{AutomationStoreError, AutomationStoreResult, required},
    fleet_row_ids::FleetRowIds,
    generated_automation_catalog::StoredAutomation,
};
use collaboration_protocol::{
    AutomationId, AutomationKind, AutomationRefusal, SessionRef, WakeCreator, WakeSendRequest,
};
use durable_steps::{StoreError, storage::TransactionScope};
use message_board::ServiceId;
use sqlx_core::{arguments::IntoArguments, database::Database, encode::Encode, types::Type};

pub(crate) enum WakeSlotReplacement {
    Healthy(Box<AutomationRecord>),
    Faulted {
        id: AutomationId,
        state: CatalogState,
    },
}

pub(crate) fn prepare(
    previous: Option<StoredAutomation>,
    request: &WakeSendRequest,
    installation: &ServiceId,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<Option<WakeSlotReplacement>> {
    let Some(row) = previous else {
        return Ok(None);
    };
    let Some(fault) = crate::automation_fault_projection::projection(&row)? else {
        return authorize_healthy(
            Some(catalog::decode(row)?),
            request,
            installation,
            now_ms,
            row_ids,
        )
        .map(|value| value.map(Box::new).map(WakeSlotReplacement::Healthy));
    };
    if fault.kind != AutomationKind::Wake {
        return Err(AutomationStoreError::InvalidRecord("slot_session_key"));
    }
    let creator_raw: serde_json::Value =
        serde_json::from_str(&required(row.creator_json, "creator_json")?)
            .map_err(|_| AutomationStoreError::InvalidRecord("creator_json"))?;
    crate::automation_request_decode::identity_limits(&creator_raw)?;
    let creator: BoundAutomationCreator = serde_json::from_value(creator_raw)
        .map_err(|_| AutomationStoreError::InvalidRecord("creator_json"))?;
    let slot = required(row.slot_session_key, "slot_session_key")?;
    let target_raw: serde_json::Value = serde_json::from_str(&slot)
        .map_err(|_| AutomationStoreError::InvalidRecord("slot_session_key"))?;
    crate::automation_request_decode::identity_limits(&target_raw)?;
    let target: SessionRef = serde_json::from_value(target_raw)
        .map_err(|_| AutomationStoreError::InvalidRecord("slot_session_key"))?;
    if target != request.message.target
        || required(row.timing_automation_id, "automation_timing.automation_id")?
            != fault.automation_id.as_str()
        || !matches!(
            row.state.as_deref(),
            Some("active" | "paused" | "suspended")
        )
    {
        return Err(AutomationStoreError::InvalidRecord("slot_session_key"));
    }
    let expiry =
        crate::automation_store_error::timestamp_millis(row.expires_at_ms, "expires_at_ms")?;
    let state = if expiry <= now_ms {
        CatalogState::Expired
    } else {
        let is_target =
            matches!(&request.requested_by, WakeCreator::Session(session) if *session == target);
        let is_creator = match &creator.declared {
            WakeCreator::Unrecorded {} => false,
            WakeCreator::Router {} => {
                creator.installation == *installation && creator.declared == request.requested_by
            }
            declared => *declared == request.requested_by,
        };
        if !request.replace
            || !(is_target || is_creator || matches!(request.requested_by, WakeCreator::Owner {}))
        {
            return Err(AutomationStoreError::Refused(
                AutomationRefusal::WakeExists {
                    creator: creator.declared,
                },
            ));
        }
        CatalogState::Replaced
    };
    Ok(Some(WakeSlotReplacement::Faulted {
        id: fault.automation_id,
        state,
    }))
}

pub(crate) async fn commit<Scope: TransactionScope>(
    scope: &mut Scope,
    replacement: WakeSlotReplacement,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let id = match replacement {
        WakeSlotReplacement::Healthy(record) => {
            catalog::update(scope, &record)
                .await
                .map_err(StoreError::backend)?;
            record.id
        }
        WakeSlotReplacement::Faulted { id, state } => {
            crate::generated_automation_admission::end_faulted_wake(
                scope,
                state.as_str().into(),
                id.as_str().into(),
            )
            .await
            .map_err(StoreError::backend)?;
            crate::generated_automation_admission::clear_automation_due(scope, id.as_str().into())
                .await
                .map_err(StoreError::backend)?;
            id
        }
    };
    crate::automation_pending_mutation::cancel(scope, &id).await
}

fn authorize_healthy(
    previous: Option<AutomationRecord>,
    request: &WakeSendRequest,
    installation: &ServiceId,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<Option<AutomationRecord>> {
    let Some(mut previous) = previous else {
        return Ok(None);
    };
    let target = match &previous.definition {
        StoredAutomationDefinition::Wake(wake) => &wake.target,
        StoredAutomationDefinition::Suspended(definition) => match &definition.configuration {
            crate::automation_catalog_record::StoredSuspendedConfiguration::Wake(wake) => {
                &wake.target
            }
            crate::automation_catalog_record::StoredSuspendedConfiguration::Schedule(_) => {
                return Err(AutomationStoreError::InvalidRecord("slot_session_key"));
            }
        },
        StoredAutomationDefinition::Schedule(_) => {
            return Err(AutomationStoreError::InvalidRecord("slot_session_key"));
        }
    };
    if timestamp_millis(previous.wake_expiry()?)? <= now_ms {
        previous.state = CatalogState::Expired;
    } else {
        let is_target =
            matches!(&request.requested_by,WakeCreator::Session(session) if session==target);
        let is_creator = match &previous.creator.declared {
            WakeCreator::Unrecorded {} => false,
            WakeCreator::Router {} => {
                previous.creator.installation == *installation
                    && previous.creator.declared == request.requested_by
            }
            creator => *creator == request.requested_by,
        };
        if !request.replace
            || !(is_target || is_creator || matches!(request.requested_by, WakeCreator::Owner {}))
        {
            return Err(AutomationStoreError::Refused(
                AutomationRefusal::WakeExists {
                    creator: previous.creator.declared,
                },
            ));
        }
        previous.state = CatalogState::Cancelled;
    }
    previous.next_due = None;
    previous.header.set_updated(now_ms)?;
    previous.revision = row_ids
        .next()?
        .to_string()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("revision"))?;
    Ok(Some(previous))
}
