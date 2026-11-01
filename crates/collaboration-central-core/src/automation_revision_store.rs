//! Immutable revision pages are scoped to their instruction, never used to choose its head.
use crate::{
    automation_content_store,
    automation_store_error::{AutomationStoreError, AutomationStoreResult, row_id, timestamp},
    generated_automation_instruction as queries,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_protocol::{
    AutomationAnswer, AutomationPage, AutomationRefusal, AutomationRequestConstraint, ContentId,
    InstructionId, RevisionListRequest, RevisionRecord,
};
use durable_steps::storage::TransactionScope;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RevisionCursor {
    instruction_id: InstructionId,
    after_content_id: ContentId,
}

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
        .map(|cursor| decode_cursor(cursor, &request.instruction_id))
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
        let cursor = RevisionCursor {
            instruction_id: request.instruction_id.clone(),
            after_content_id: last.reference.content_id.clone(),
        };
        Some(
            URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&cursor)
                    .map_err(|_| AutomationStoreError::InvalidRecord("revision_cursor"))?,
            ),
        )
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

fn decode_cursor(cursor: &str, instruction_id: &InstructionId) -> AutomationStoreResult<ContentId> {
    let invalid = || AutomationStoreError::InvalidRequest {
        field: "cursor",
        reason: AutomationRequestConstraint::InvalidCursor,
    };
    let bytes = URL_SAFE_NO_PAD.decode(cursor).map_err(|_| invalid())?;
    let decoded: RevisionCursor = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if decoded.instruction_id != *instruction_id {
        return Err(invalid());
    }
    Ok(decoded.after_content_id)
}
