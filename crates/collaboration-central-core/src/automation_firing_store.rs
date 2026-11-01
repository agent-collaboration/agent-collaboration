//! Inspection reads the admitted content identity and central outcome without local IDs.
use crate::{
    automation_list_cursor::{AutomationListCursor, invalid_cursor},
    automation_store_error::{
        AutomationStoreError, AutomationStoreResult, required, timestamp_millis,
    },
    generated_automation_firing as queries,
};
use collaboration_protocol::{
    AutomationId, ContentReference, FiringId, FiringOutcome, FiringSnapshot, GridTime,
    WorkReportSequence,
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
    let sequence: Option<WorkReportSequence> = row
        .report_seq
        .map(|value| {
            let sequence = value
                .parse::<u64>()
                .map_err(|_| AutomationStoreError::InvalidRecord("report_seq"))?;
            if sequence == 0 {
                return Err(AutomationStoreError::InvalidRecord("report_seq"));
            }
            sequence.try_into().map_err(AutomationStoreError::from)
        })
        .transpose()?;
    let report_owner = row
        .report_service_id
        .map(|value| {
            message_board::ServiceId::try_from(value)
                .map_err(|_| AutomationStoreError::InvalidRecord("report_service_id"))
        })
        .transpose()?;
    if report_owner.is_none()
        && (row.batch_id.is_some()
            || sequence.is_some()
            || !matches!(outcome, FiringOutcome::Pending | FiringOutcome::Cancelled))
    {
        return Err(AutomationStoreError::InvalidRecord("report_service_id"));
    }
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
        report_seq: sequence,
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

pub(crate) async fn list<Scope: TransactionScope>(
    scope: &mut Scope,
    request: &collaboration_protocol::FiringListRequest,
) -> AutomationStoreResult<collaboration_protocol::AutomationAnswer>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let after = request
        .cursor
        .as_deref()
        .map(|cursor| match AutomationListCursor::decode(cursor)? {
            AutomationListCursor::Firings {
                automation_id,
                after,
            } if automation_id == request.automation_id => Ok(after),
            _ => Err(invalid_cursor()),
        })
        .transpose()?;
    if let Some(id) = &after {
        let snapshot = read(scope, id).await?.ok_or_else(invalid_cursor)?;
        if snapshot.automation_id != request.automation_id {
            return Err(invalid_cursor());
        }
    }
    let limit = u32::from(request.limit) as usize;
    let rows = queries::list_automation_firings(
        scope,
        request.automation_id.as_str().into(),
        after.map(String::from).unwrap_or_default(),
        (limit + 1).to_string(),
    )
    .await?;
    let more = rows.len() > limit;
    let records = rows
        .into_iter()
        .take(limit)
        .map(decode)
        .collect::<AutomationStoreResult<Vec<_>>>()?;
    if records
        .iter()
        .any(|row| row.automation_id != request.automation_id)
    {
        return Err(AutomationStoreError::InvalidRecord("automation_id"));
    }
    let next_cursor = if more {
        Some(
            AutomationListCursor::Firings {
                automation_id: request.automation_id.clone(),
                after: records
                    .last()
                    .ok_or(AutomationStoreError::InvalidRecord("firing.page"))?
                    .firing_id
                    .clone(),
            }
            .encode()?,
        )
    } else {
        None
    };
    Ok(collaboration_protocol::AutomationAnswer::Firings(
        collaboration_protocol::AutomationPage {
            records,
            next_cursor,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unreported_row(sequence: Option<&str>) -> queries::StoredAutomationFiring {
        let identity = || Some(durable_steps::Uuid::now_v7().to_string());
        queries::StoredAutomationFiring {
            firing_id: identity(),
            automation_id: identity(),
            due_ms: Some("1793440800000".into()),
            revision: identity(),
            content_id: identity(),
            content_revision_id: identity(),
            content_digest: Some("a".repeat(64)),
            state: Some("pending".into()),
            batch_id: None,
            report_seq: sequence.map(str::to_owned),
            outcome_json: None,
            report_service_id: None,
        }
    }

    #[test]
    fn null_sequence_is_unreported() {
        let snapshot = decode(unreported_row(None)).expect("NULL is the unreported state");
        assert_eq!(snapshot.report_seq, None);
    }

    #[test]
    fn stored_zero_sequence_is_refused_instead_of_coerced() {
        assert!(matches!(
            decode(unreported_row(Some("0"))),
            Err(AutomationStoreError::InvalidRecord("report_seq"))
        ));
    }

    #[test]
    fn positive_sequence_requires_the_retained_report_owner() {
        let mut row = unreported_row(Some("1"));
        assert!(matches!(
            decode(unreported_row(Some("1"))),
            Err(AutomationStoreError::InvalidRecord("report_service_id"))
        ));
        row.report_service_id = Some(durable_steps::Uuid::now_v7().to_string());
        assert_eq!(
            decode(row)
                .expect("positive owned report")
                .report_seq
                .map(u64::from),
            Some(1)
        );
    }
}
