//! Catalog records and timer progress are decoded explicitly at the Fleet owner.
use crate::{
    automation_catalog_record::{
        AutomationProgress, AutomationRecord, BoundAutomationCreator, CatalogState,
        StoredAutomationDefinition, validate_timing, wake_slot,
    },
    automation_record_header::{
        AutomationRecordHeader, HeaderColumnValues, ScheduleExecutionMode, StoredHeaderFields,
    },
    automation_store_error::{
        AutomationStoreError, AutomationStoreResult, observation_millis, required, row_id,
        timestamp, timestamp_millis,
    },
    generated_automation_catalog as queries,
};
use collaboration_protocol::{AutomationId, AutomationKind, ChangeId, FiringId, GridTime};
use durable_steps::storage::TransactionScope;
use serde::Serialize;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) fn decode(row: queries::StoredAutomation) -> AutomationStoreResult<AutomationRecord> {
    if row.fault_json.is_some() || row.faulted_at_ms.is_some() {
        return Err(AutomationStoreError::InvalidRecord("faulted_automation"));
    }
    let kind = match row.kind.as_deref() {
        Some("schedule") => AutomationKind::Schedule,
        Some("wake") => AutomationKind::Wake,
        _ => return Err(AutomationStoreError::InvalidRecord("kind")),
    };
    let header = decode_header(&row, kind)?;
    let id: AutomationId = required(row.automation_id, "automation_id")?
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("automation_id"))?;
    if required(row.timing_automation_id, "automation_timing.automation_id")? != id.as_str() {
        return Err(AutomationStoreError::InvalidRecord(
            "automation_timing.automation_id",
        ));
    }
    let revision: ChangeId = required(row.revision, "revision")?
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("revision"))?;
    let expires_at = optional_timestamp(row.expires_at_ms, "expires_at_ms")?
        .map(timestamp)
        .transpose()?;
    let raw: serde_json::Value =
        serde_json::from_str(&required(row.definition_json, "definition_json")?)
            .map_err(|_| AutomationStoreError::InvalidRecord("definition_json"))?;
    crate::automation_request_decode::identity_limits(&raw)?;
    crate::automation_request_decode::counter_limits(&raw)?;
    let definition: StoredAutomationDefinition = serde_json::from_value(raw)
        .map_err(|_| AutomationStoreError::InvalidRecord("definition_json"))?;
    if row.legacy_timing_json.is_some() {
        return Err(AutomationStoreError::InvalidRecord("legacy_timing_json"));
    }
    let state: CatalogState =
        serde_json::from_value(serde_json::Value::String(required(row.state, "state")?))
            .map_err(|_| AutomationStoreError::InvalidRecord("state"))?;
    let raw_creator: serde_json::Value =
        serde_json::from_str(&required(row.creator_json, "creator_json")?)
            .map_err(|_| AutomationStoreError::InvalidRecord("creator_json"))?;
    crate::automation_request_decode::identity_limits(&raw_creator)?;
    let creator: BoundAutomationCreator = serde_json::from_value(raw_creator)
        .map_err(|_| AutomationStoreError::InvalidRecord("creator_json"))?;
    let next_due = optional_grid(row.next_due_ms, "next_due_ms")?;
    let admitted_through = optional_grid(row.admitted_through_ms, "admitted_through_ms")?;
    optional_timestamp(
        row.imported_watermark_local_ms,
        "imported_watermark_local_ms",
    )?;
    // Runtime occupancy is populated by admission; its IDs remain validated even
    // when no local Run ID has been reported for the legacy snapshot yet.
    let occupying = optional_firing(row.occupying_firing_id)?;
    let waiting = optional_firing(row.waiting_firing_id)?;
    let external = row
        .occupying_external_run
        .map(|value| {
            row_id(&value, "occupying_external_run")?;
            value
                .try_into()
                .map_err(|_| AutomationStoreError::InvalidRecord("occupying_external_run"))
        })
        .transpose()?;
    match &definition {
        StoredAutomationDefinition::Schedule(schedule) => {
            if kind != AutomationKind::Schedule
                || !matches!(
                    state,
                    CatalogState::Enabled | CatalogState::Disabled | CatalogState::Deleted
                )
                || schedule.definition.enabled != (state == CatalogState::Enabled)
                || row.slot_session_key.is_some()
            {
                return Err(AutomationStoreError::InvalidRecord("schedule.state"));
            }
            let AutomationRecordHeader::Schedule { execution_mode, .. } = &header else {
                return Err(AutomationStoreError::InvalidRecord(
                    "schedule.record_header",
                ));
            };
            if *execution_mode
                != ScheduleExecutionMode::for_destination(&schedule.definition.destination)
            {
                return Err(AutomationStoreError::InvalidRecord("execution_mode"));
            }
            if expires_at.is_some() {
                return Err(AutomationStoreError::InvalidRecord("expires_at_ms"));
            }
            validate_timing(
                &schedule.definition.timing,
                &schedule.timing,
                AutomationKind::Schedule,
            )?;
            crate::automation_schedule_validation::definition(&schedule.definition)?;
            if state != CatalogState::Enabled && next_due.is_some() {
                return Err(AutomationStoreError::InvalidRecord("next_due_ms"));
            }
        }
        StoredAutomationDefinition::Wake(wake) => {
            if kind != AutomationKind::Wake
                || matches!(
                    state,
                    CatalogState::Enabled | CatalogState::Disabled | CatalogState::Deleted
                )
            {
                return Err(AutomationStoreError::InvalidRecord("wake.state"));
            }
            validate_timing(&wake.timing_request, &wake.timing, AutomationKind::Wake)?;
            crate::automation_wake_validation::expiry(
                &collaboration_protocol::ExpiryRequest::At {
                    at: expires_at
                        .clone()
                        .ok_or(AutomationStoreError::InvalidRecord("expires_at_ms"))?,
                },
                &wake.timing_request,
                &wake.timing,
                observation_millis(&header.times().created_at)?,
            )?;
            let expected_slot = if state.owns_wake_slot() {
                Some(wake_slot(&wake.target)?)
            } else {
                None
            };
            if row.slot_session_key != expected_slot {
                return Err(AutomationStoreError::InvalidRecord("slot_session_key"));
            }
            if state != CatalogState::Active && next_due.is_some() {
                return Err(AutomationStoreError::InvalidRecord("next_due_ms"));
            }
        }
    }
    Ok(AutomationRecord {
        id,
        revision,
        definition,
        state,
        creator,
        next_due,
        header,
        expires_at,
        progress: AutomationProgress {
            admitted_through,
            occupying,
            waiting,
            external,
        },
    })
}

pub(crate) fn decode_header(
    row: &queries::StoredAutomation,
    kind: AutomationKind,
) -> AutomationStoreResult<AutomationRecordHeader> {
    AutomationRecordHeader::decode(
        kind,
        StoredHeaderFields {
            anchor_at_ms: row.anchor_at_ms.clone(),
            created_at_ms: row.created_at_ms.clone(),
            updated_at_ms: row.updated_at_ms.clone(),
            imported_continuity_json: row.imported_continuity_json.clone(),
            execution_mode: row.execution_mode.clone(),
        },
    )
}

fn optional_firing(value: Option<String>) -> AutomationStoreResult<Option<FiringId>> {
    value
        .map(|value| {
            value
                .try_into()
                .map_err(|_| AutomationStoreError::InvalidRecord("firing_id"))
        })
        .transpose()
}

fn optional_timestamp(
    value: Option<String>,
    field: &'static str,
) -> AutomationStoreResult<Option<i64>> {
    value
        .map(|value| timestamp_millis(Some(value), field))
        .transpose()
}

fn optional_grid(
    value: Option<String>,
    field: &'static str,
) -> AutomationStoreResult<Option<GridTime>> {
    value
        .map(|value| Ok(GridTime::new(timestamp_millis(Some(value), field)?)?))
        .transpose()
}

pub(crate) async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    id: &AutomationId,
) -> AutomationStoreResult<Option<AutomationRecord>>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let record = queries::read_automation(scope, id.as_str().into())
        .await?
        .map(decode)
        .transpose()?;
    if record.as_ref().is_some_and(|record| record.id != *id) {
        return Err(AutomationStoreError::InvalidRecord("automation_id"));
    }
    Ok(record)
}

pub(crate) async fn insert<Scope: TransactionScope>(
    scope: &mut Scope,
    record: &AutomationRecord,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let definition = encode(&record.definition)?;
    let creator = serde_json::to_string(&record.creator)
        .map_err(|_| AutomationStoreError::InvalidRecord("creator_json"))?;
    let input = AutomationInsert {
        automation_id: record.id.as_str(),
        revision: record.revision.as_str(),
        kind: match record.definition {
            StoredAutomationDefinition::Schedule(_) => AutomationKind::Schedule,
            StoredAutomationDefinition::Wake(_) => AutomationKind::Wake,
        },
        definition_json: definition,
        creator_json: creator,
        state: record.state,
        slot_session_key: slot(record)?,
        header: record.header.columns()?,
    };
    let input = serde_json::to_string(&input)
        .map_err(|_| AutomationStoreError::InvalidRecord("automation"))?;
    let expiry = record
        .expires_at
        .as_ref()
        .map(observation_millis)
        .transpose()?
        .map(|value| value.to_string());
    queries::insert_automation(scope, input).await?;
    queries::insert_automation_timing(
        scope,
        record.id.as_str().into(),
        record.next_due.map(|due| due.unix_millis().to_string()),
        expiry,
    )
    .await?;
    Ok(())
}

pub(crate) async fn update<Scope: TransactionScope>(
    scope: &mut Scope,
    record: &AutomationRecord,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let definition = encode(&record.definition)?;
    let slot = slot(record)?;
    let updated = observation_millis(&record.header.times().updated_at)?.to_string();
    let expiry = record
        .expires_at
        .as_ref()
        .map(observation_millis)
        .transpose()?
        .map(|value| value.to_string());
    queries::update_automation(
        scope,
        record.revision.as_str().into(),
        definition,
        record.state.as_str().into(),
        slot,
        updated,
        record.id.as_str().into(),
    )
    .await?;
    queries::update_automation_deadline(
        scope,
        record.next_due.map(|due| due.unix_millis().to_string()),
        expiry,
        record.id.as_str().into(),
    )
    .await?;
    Ok(())
}

#[derive(Serialize)]
struct AutomationInsert<'record> {
    automation_id: &'record str,
    kind: AutomationKind,
    revision: &'record str,
    definition_json: String,
    state: CatalogState,
    creator_json: String,
    slot_session_key: Option<String>,
    #[serde(flatten)]
    header: HeaderColumnValues,
}
fn encode(definition: &StoredAutomationDefinition) -> AutomationStoreResult<String> {
    serde_json::to_string(definition)
        .map_err(|_| AutomationStoreError::InvalidRecord("definition_json"))
}
fn slot(record: &AutomationRecord) -> AutomationStoreResult<Option<String>> {
    match &record.definition {
        StoredAutomationDefinition::Wake(wake) if record.state.owns_wake_slot() => {
            Ok(Some(wake_slot(&wake.target)?))
        }
        _ => Ok(None),
    }
}
