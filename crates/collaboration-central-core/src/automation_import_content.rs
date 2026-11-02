//! Imported immutable revisions are checked before either content owner is written.
use crate::{
    automation_content_store as contents, automation_instruction_store as instructions,
    automation_store_error::{AutomationStoreError, AutomationStoreResult, row_id},
    fleet_row_ids::FleetRowIds,
    generated_automation_content as queries, generated_automation_instruction as documents,
};
use collaboration_protocol::{
    AutomationImportDefinition, AutomationImportPayload, AutomationRefusal, ContentReference,
    InstructionId, SuspendedAutomationConfiguration, limits::LimitName,
};
use durable_steps::{Uuid, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) struct ImportedContent {
    pub reference: ContentReference,
    owner: Uuid,
    insert_revision: bool,
    insert_document: Option<InstructionId>,
}

pub(crate) fn conflict(payload: &AutomationImportPayload) -> AutomationStoreError {
    AutomationStoreError::Refused(AutomationRefusal::ImportConflict {
        automation_id: payload.automation_id.clone(),
    })
}

pub(crate) async fn prepare<Scope: TransactionScope>(
    scope: &mut Scope,
    payload: &AutomationImportPayload,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<ImportedContent>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let instruction = match &payload.definition {
        AutomationImportDefinition::Schedule(definition) => Some(&definition.instruction_id),
        AutomationImportDefinition::Suspended(definition) => match &definition.configuration {
            SuspendedAutomationConfiguration::Schedule(configuration) => {
                Some(&configuration.instruction_id)
            }
            SuspendedAutomationConfiguration::Wake(_) => None,
        },
        AutomationImportDefinition::Wake(_) => None,
    };
    let owner = row_id(
        instruction.map_or(payload.automation_id.as_str(), InstructionId::as_str),
        "content.owner_id",
    )?;
    let digest = contents::text_digest(payload.content.text.as_str())?;
    let existing = queries::read_imported_content_revision(
        scope,
        owner.to_string(),
        payload.content.revision_id.as_str().into(),
    )
    .await?
    .map(|row| contents::decode(row, LimitName::InstructionMaxBytes))
    .transpose()?;
    let reference = if let Some(existing) = &existing {
        if existing.owner_id != owner
            || existing.reference.revision_id != payload.content.revision_id
            || existing.text != payload.content.text
            || existing.reference.digest != digest
        {
            return Err(conflict(payload));
        }
        existing.reference.clone()
    } else {
        ContentReference {
            content_id: row_ids
                .next()?
                .to_string()
                .try_into()
                .map_err(|_| AutomationStoreError::InvalidRecord("content_id"))?,
            revision_id: payload.content.revision_id.clone(),
            digest,
        }
    };
    let insert_document = if let Some(instruction) = instruction {
        match instructions::read(scope, instruction).await? {
            Some(current)
                if current.revision_id != payload.content.revision_id
                    || current.text != payload.content.text =>
            {
                return Err(conflict(payload));
            }
            Some(_) => None,
            None => Some(instruction.clone()),
        }
    } else {
        None
    };
    Ok(ImportedContent {
        reference,
        owner,
        insert_revision: existing.is_none(),
        insert_document,
    })
}

pub(crate) async fn commit<Scope: TransactionScope>(
    scope: &mut Scope,
    prepared: &ImportedContent,
    payload: &AutomationImportPayload,
    now_ms: i64,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    if prepared.insert_revision {
        contents::insert_prepared(
            scope,
            prepared.owner,
            &prepared.reference,
            &payload.content.text,
            now_ms,
        )
        .await?;
    }
    if let Some(instruction) = &prepared.insert_document {
        documents::insert_instruction_document(
            scope,
            instruction.as_str().into(),
            prepared.reference.content_id.as_str().into(),
            now_ms.to_string(),
            now_ms.to_string(),
        )
        .await?;
    }
    Ok(())
}
