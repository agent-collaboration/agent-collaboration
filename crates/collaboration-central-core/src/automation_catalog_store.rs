//! Catalog records and timer progress are decoded explicitly at the Fleet owner.
use crate::{
    automation_catalog_record::{
        AutomationRecord, BoundAutomationCreator, CatalogState, StoredAutomationDefinition,
        validate_timing, wake_slot,
    },
    automation_store_error::{
        AutomationStoreError, AutomationStoreResult, required, row_id, timestamp_millis,
    },
    generated_automation_catalog as queries,
};
use collaboration_protocol::{AutomationId, AutomationKind, ChangeId, FiringId, GridTime};
use durable_steps::storage::TransactionScope;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) fn decode(row: queries::StoredAutomation) -> AutomationStoreResult<AutomationRecord> {
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
    optional_timestamp(row.admitted_through_ms, "admitted_through_ms")?;
    optional_timestamp(
        row.imported_watermark_local_ms,
        "imported_watermark_local_ms",
    )?;
    // Runtime occupancy is populated by admission; its IDs remain validated even
    // when no local Run ID has been reported for the legacy snapshot yet.
    for value in [row.occupying_firing_id, row.waiting_firing_id]
        .into_iter()
        .flatten()
    {
        let _: FiringId = value
            .try_into()
            .map_err(|_| AutomationStoreError::InvalidRecord("firing_id"))?;
    }
    if let Some(external) = row.occupying_external_run {
        row_id(&external, "occupying_external_run")?;
    }
    let kind = required(row.kind, "kind")?;
    match &definition {
        StoredAutomationDefinition::Schedule(schedule) => {
            if kind != "schedule"
                || !matches!(
                    state,
                    CatalogState::Enabled | CatalogState::Disabled | CatalogState::Deleted
                )
                || schedule.definition.enabled != (state == CatalogState::Enabled)
                || row.slot_session_key.is_some()
            {
                return Err(AutomationStoreError::InvalidRecord("schedule.state"));
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
            if kind != "wake"
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
                    at: wake.expires_at.clone(),
                },
                &wake.timing_request,
                &wake.timing,
                crate::automation_catalog_record::timestamp_millis(&wake.created_at)?,
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
    })
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
    match &record.definition {
        StoredAutomationDefinition::Schedule(_) => {
            queries::insert_schedule_automation(
                scope,
                record.id.as_str().into(),
                record.revision.as_str().into(),
                definition,
                record.state.as_str().into(),
                creator,
            )
            .await?;
        }
        StoredAutomationDefinition::Wake(_) => {
            queries::insert_wake_automation(
                scope,
                record.id.as_str().into(),
                record.revision.as_str().into(),
                definition,
                record.state.as_str().into(),
                creator,
                slot(record)?,
            )
            .await?;
        }
    }
    queries::insert_automation_timing(
        scope,
        record.id.as_str().into(),
        record.next_due.map(|due| due.unix_millis().to_string()),
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
    queries::update_automation(
        scope,
        record.revision.as_str().into(),
        definition,
        record.state.as_str().into(),
        slot,
        record.id.as_str().into(),
    )
    .await?;
    queries::update_automation_deadline(
        scope,
        record.next_due.map(|due| due.unix_millis().to_string()),
        record.id.as_str().into(),
    )
    .await?;
    Ok(())
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
