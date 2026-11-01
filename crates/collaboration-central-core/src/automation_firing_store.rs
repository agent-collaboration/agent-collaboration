//! Inspection reads the admitted content identity and central outcome without local IDs.
use crate::{
    automation_store_error::{
        AutomationStoreError, AutomationStoreResult, required, timestamp_millis,
    },
    generated_automation_firing as queries,
};
use collaboration_protocol::{
    AutomationId, ContentReference, FiringId, FiringOutcome, FiringSnapshot, GridTime,
};
use durable_steps::storage::TransactionScope;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) fn decode(
    row: queries::StoredAutomationFiring,
) -> AutomationStoreResult<FiringSnapshot> {
    let state = required(row.state, "automation_firings.state")?;
    let raw = match row.outcome_json {
        Some(raw) => serde_json::from_str(&raw)
            .map_err(|_| AutomationStoreError::InvalidRecord("outcome_json"))?,
        None => serde_json::json!({"state":state}),
    };
    crate::automation_request_decode::outcome_limits(&raw)?;
    let outcome: FiringOutcome = serde_json::from_value(raw)
        .map_err(|_| AutomationStoreError::InvalidRecord("outcome_json"))?;
    let encoded = serde_json::to_value(&outcome)
        .map_err(|_| AutomationStoreError::InvalidRecord("outcome_json"))?;
    if encoded.get("state").and_then(serde_json::Value::as_str) != Some(state.as_str()) {
        return Err(AutomationStoreError::InvalidRecord(
            "automation_firings.state",
        ));
    }
    let sequence = required(row.report_seq, "report_seq")?
        .parse::<u64>()
        .map_err(|_| AutomationStoreError::InvalidRecord("report_seq"))?;
    Ok(FiringSnapshot {
        firing_id: required(row.firing_id, "firing_id")?
            .try_into()
            .map_err(|_| AutomationStoreError::InvalidRecord("firing_id"))?,
        automation_id: required(row.automation_id, "automation_id")?
            .try_into()
            .map_err(|_| AutomationStoreError::InvalidRecord("automation_id"))?,
        due_at: GridTime::new(timestamp_millis(row.due_ms, "due_ms")?)?,
        revision: required(row.revision, "revision")?
            .try_into()
            .map_err(|_| AutomationStoreError::InvalidRecord("revision"))?,
        content: ContentReference {
            content_id: required(row.content_id, "content_id")?
                .try_into()
                .map_err(|_| AutomationStoreError::InvalidRecord("content_id"))?,
            revision_id: required(row.content_revision_id, "content_revision_id")?
                .try_into()
                .map_err(|_| AutomationStoreError::InvalidRecord("content_revision_id"))?,
            digest: required(row.content_digest, "content_digest")?
                .try_into()
                .map_err(|_| AutomationStoreError::InvalidRecord("content_digest"))?,
        },
        outcome,
        report_seq: if sequence == 0 {
            None
        } else {
            Some(sequence.try_into()?)
        },
        batch_id: row
            .batch_id
            .map(|id| {
                id.try_into()
                    .map_err(|_| AutomationStoreError::InvalidRecord("batch_id"))
            })
            .transpose()?,
    })
}

pub(crate) async fn first<Scope: TransactionScope>(
    scope: &mut Scope,
    id: &AutomationId,
) -> AutomationStoreResult<Option<FiringSnapshot>>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let firing = queries::first_automation_firing(scope, id.as_str().into())
        .await?
        .map(decode)
        .transpose()?;
    if firing
        .as_ref()
        .is_some_and(|firing| firing.automation_id != *id)
    {
        return Err(AutomationStoreError::InvalidRecord("automation_id"));
    }
    Ok(firing)
}

pub(crate) async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    id: &FiringId,
) -> AutomationStoreResult<Option<FiringSnapshot>>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let firing = queries::read_automation_firing(scope, id.as_str().into())
        .await?
        .map(decode)
        .transpose()?;
    if firing
        .as_ref()
        .is_some_and(|firing| firing.firing_id != *id)
    {
        return Err(AutomationStoreError::InvalidRecord("firing_id"));
    }
    Ok(firing)
}

pub(crate) async fn sent_firings<Scope: TransactionScope>(
    scope: &mut Scope,
    id: &AutomationId,
) -> AutomationStoreResult<Vec<FiringId>>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    queries::automation_sent_firings(scope, id.as_str().into())
        .await?
        .into_iter()
        .map(|row| {
            required(row.firing_id, "firing_id")?
                .try_into()
                .map_err(|_| AutomationStoreError::InvalidRecord("firing_id"))
        })
        .collect()
}
