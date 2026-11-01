//! An instruction's explicit head selects an immutable content revision.
use crate::{
    automation_content_store,
    automation_list_cursor::{AutomationListCursor, invalid_cursor},
    automation_store_error::{
        AutomationStoreError, AutomationStoreResult, required, row_id, timestamp, timestamp_millis,
    },
    fleet_row_ids::FleetRowIds,
    generated_automation_instruction as queries,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationPage, AutomationPageRequest, AutomationRefusal, ContentId,
    InstructionId, InstructionSnapshot, InstructionText, InstructionUpdateParams,
};
use durable_steps::storage::TransactionScope;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    instruction_id: &InstructionId,
) -> AutomationStoreResult<Option<InstructionSnapshot>>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let Some(row) =
        queries::read_instruction_document(scope, instruction_id.as_str().into()).await?
    else {
        return Ok(None);
    };
    let snapshot = decode_document(scope, row).await?;
    if snapshot.instruction_id != *instruction_id {
        return Err(AutomationStoreError::InvalidRecord("instruction_id"));
    }
    Ok(Some(snapshot))
}

async fn decode_document<Scope: TransactionScope>(
    scope: &mut Scope,
    row: queries::StoredInstructionDocument,
) -> AutomationStoreResult<InstructionSnapshot>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let instruction_id: InstructionId = required(row.instruction_id, "instruction_id")?
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("instruction_id"))?;
    let content_id: ContentId = required(row.current_content_id, "current_content_id")?
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("current_content_id"))?;
    let content = automation_content_store::read(scope, &content_id)
        .await?
        .ok_or(AutomationStoreError::InvalidRecord("current_content_id"))?;
    if content.owner_id != row_id(instruction_id.as_str(), "instruction_id")? {
        return Err(AutomationStoreError::InvalidRecord("content.owner_id"));
    }
    Ok(InstructionSnapshot {
        instruction_id,
        revision_id: content.reference.revision_id,
        text: content.text,
        created_at: timestamp(timestamp_millis(row.created_at_ms, "created_at_ms")?)?,
        updated_at: timestamp(timestamp_millis(row.updated_at_ms, "updated_at_ms")?)?,
    })
}

pub(crate) async fn create<Scope: TransactionScope>(
    scope: &mut Scope,
    text: &InstructionText,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<InstructionSnapshot>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let owner_id = row_ids.next()?;
    let instruction_id: InstructionId = owner_id
        .to_string()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("instruction_id"))?;
    insert_with_id(scope, &instruction_id, text, now_ms, row_ids).await
}

pub(crate) async fn insert_with_id<Scope: TransactionScope>(
    scope: &mut Scope,
    instruction_id: &InstructionId,
    text: &InstructionText,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<InstructionSnapshot>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let now = timestamp(now_ms)?;
    let owner_id = row_id(instruction_id.as_str(), "instruction_id")?;
    let content = automation_content_store::insert(scope, owner_id, text, now_ms, row_ids).await?;
    queries::insert_instruction_document(
        scope,
        instruction_id.as_str().into(),
        content.content_id.as_str().into(),
        now_ms.to_string(),
        now_ms.to_string(),
    )
    .await?;
    Ok(InstructionSnapshot {
        instruction_id: instruction_id.clone(),
        revision_id: content.revision_id,
        text: text.clone(),
        created_at: now.clone(),
        updated_at: now,
    })
}

pub(crate) async fn update<Scope: TransactionScope>(
    scope: &mut Scope,
    request: &InstructionUpdateParams,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<AutomationAnswer>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let Some(current) = read(scope, &request.instruction_id).await? else {
        return Ok(AutomationAnswer::Refused(AutomationRefusal::NotFound));
    };
    if current.revision_id != request.expected_revision_id {
        return Ok(AutomationAnswer::Refused(
            AutomationRefusal::InstructionRevisionConflict {
                current_revision_id: current.revision_id,
            },
        ));
    }
    let now = timestamp(now_ms)?;
    let owner_id = row_id(request.instruction_id.as_str(), "instruction_id")?;
    let content =
        automation_content_store::insert(scope, owner_id, &request.text, now_ms, row_ids).await?;
    queries::move_instruction_head(
        scope,
        content.content_id.as_str().into(),
        now_ms.to_string(),
        request.instruction_id.as_str().into(),
    )
    .await?;
    Ok(AutomationAnswer::Instruction(InstructionSnapshot {
        instruction_id: request.instruction_id.clone(),
        revision_id: content.revision_id,
        text: request.text.clone(),
        created_at: current.created_at,
        updated_at: now,
    }))
}

pub(crate) async fn list<Scope: TransactionScope>(
    scope: &mut Scope,
    request: &AutomationPageRequest,
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
            AutomationListCursor::Instructions { after } => Ok(after),
            _ => Err(invalid_cursor()),
        })
        .transpose()?;
    let limit = usize::try_from(u32::from(request.limit))
        .map_err(|_| AutomationStoreError::InvalidRecord("page_limit"))?;
    let rows = queries::list_instruction_documents(
        scope,
        after.map(String::from).unwrap_or_default(),
        (limit + 1).to_string(),
    )
    .await?;
    let more = rows.len() > limit;
    let mut records = Vec::new();
    for row in rows.into_iter().take(limit) {
        records.push(decode_document(scope, row).await?);
    }
    let next_cursor = if more {
        Some(
            AutomationListCursor::Instructions {
                after: records
                    .last()
                    .ok_or(AutomationStoreError::InvalidRecord("instruction_page"))?
                    .instruction_id
                    .clone(),
            }
            .encode()?,
        )
    } else {
        None
    };
    Ok(AutomationAnswer::Instructions(AutomationPage {
        records,
        next_cursor,
    }))
}
