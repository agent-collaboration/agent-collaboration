//! Immutable revision pages are scoped to their instruction, never used to choose its head.
use crate::{
    automation_content_store,
    automation_list_cursor::{AutomationListCursor, invalid_cursor},
    automation_store_error::{AutomationStoreError, AutomationStoreResult, row_id, timestamp},
    generated_automation_instruction as queries,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationPage, AutomationRefusal, RevisionListRequest, RevisionRecord,
};
use durable_steps::storage::TransactionScope;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn list<Scope: TransactionScope>(
    scope: &mut Scope,
    request: &RevisionListRequest,
) -> AutomationStoreResult<AutomationAnswer>
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
            AutomationListCursor::Revisions {
                instruction_id,
                after,
            } if instruction_id == request.instruction_id => Ok(after),
            _ => Err(invalid_cursor()),
        })
        .transpose()?;
    if !queries::instruction_document_exists(scope, request.instruction_id.as_str().into()).await? {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    }
    let limit = usize::try_from(u32::from(request.limit))
        .map_err(|_| AutomationStoreError::InvalidRecord("page_limit"))?;
    let rows = automation_content_store::list(
        scope,
        row_id(request.instruction_id.as_str(), "instruction_id")?,
        after.as_ref(),
        limit + 1,
    )
    .await?;
    let more = rows.len() > limit;
    let rows: Vec<_> = rows.into_iter().take(limit).collect();
    let next_cursor = if more {
        let last = rows
            .last()
            .ok_or(AutomationStoreError::InvalidRecord("revision_page"))?;
        let cursor = AutomationListCursor::Revisions {
            instruction_id: request.instruction_id.clone(),
            after: last.reference.content_id.clone(),
        };
        Some(cursor.encode()?)
    } else {
        None
    };
    let records = rows
        .into_iter()
        .map(|content| {
            Ok(RevisionRecord {
                revision_id: content.reference.revision_id,
                instruction_id: request.instruction_id.clone(),
                recorded_at: timestamp(content.recorded_at_ms)?,
                source_revision_id: None,
                text: String::from(content.text),
            })
        })
        .collect::<AutomationStoreResult<Vec<_>>>()?;
    Ok(AutomationAnswer::Revisions(AutomationPage {
        records,
        next_cursor,
    }))
}
