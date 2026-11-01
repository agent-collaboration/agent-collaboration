//! Immutable text revisions share the caller's Fleet transaction.
use crate::{
    automation_store_error::{
        AutomationStoreError, AutomationStoreResult, required, row_id, timestamp_millis,
    },
    fleet_row_ids::FleetRowIds,
    generated_automation_content as queries,
};
use collaboration_mail_protocol::MailDigest;
use collaboration_protocol::{
    ContentId, ContentReference, InstructionText, RevisionId, limits::LimitName,
};
use durable_steps::{Uuid, storage::TransactionScope};
use sha2::{Digest, Sha256};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) struct ContentRevision {
    pub owner_id: Uuid,
    pub reference: ContentReference,
    pub text: InstructionText,
    pub recorded_at_ms: i64,
}

pub(crate) fn text_digest(text: &str) -> AutomationStoreResult<MailDigest> {
    format!("{:x}", Sha256::digest(text.as_bytes()))
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("digest"))
}

pub(crate) async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    content_id: &ContentId,
) -> AutomationStoreResult<Option<ContentRevision>>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let Some(row) = queries::read_content_revision(scope, content_id.as_str().into()).await? else {
        return Ok(None);
    };
    let content = decode(row, LimitName::InstructionMaxBytes)?;
    if content.reference.content_id != *content_id {
        return Err(AutomationStoreError::InvalidRecord("content_id"));
    }
    Ok(Some(content))
}

fn decode(
    row: queries::StoredContentRevision,
    limit: LimitName,
) -> AutomationStoreResult<ContentRevision> {
    let stored_id: ContentId = required(row.content_id, "content_id")?
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("content_id"))?;
    let owner_id = row_id(&required(row.owner_id, "owner_id")?, "owner_id")?;
    let revision_id: RevisionId = required(row.revision_id, "revision_id")?
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("revision_id"))?;
    let text = required(row.text, "text")?;
    limit.validate(text.len() as u64)?;
    let text: InstructionText = text
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("text"))?;
    let digest: MailDigest = required(row.digest, "digest")?
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("digest"))?;
    if digest != text_digest(text.as_str())? {
        return Err(AutomationStoreError::InvalidRecord("digest"));
    }
    Ok(ContentRevision {
        owner_id,
        reference: ContentReference {
            content_id: stored_id,
            revision_id,
            digest,
        },
        text,
        recorded_at_ms: timestamp_millis(row.recorded_at_ms, "recorded_at_ms")?,
    })
}

pub(crate) async fn list<Scope: TransactionScope>(
    scope: &mut Scope,
    owner_id: Uuid,
    after: Option<&ContentId>,
    limit: usize,
) -> AutomationStoreResult<Vec<ContentRevision>>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let rows = queries::list_content_revisions(
        scope,
        owner_id.to_string(),
        after.map(|id| id.as_str().to_owned()).unwrap_or_default(),
        limit.to_string(),
    )
    .await?;
    rows.into_iter()
        .map(|row| {
            let content = decode(row, LimitName::InstructionMaxBytes)?;
            if content.owner_id != owner_id {
                return Err(AutomationStoreError::InvalidRecord("owner_id"));
            }
            Ok(content)
        })
        .collect()
}

pub(crate) async fn insert<Scope: TransactionScope>(
    scope: &mut Scope,
    owner_id: Uuid,
    text: &InstructionText,
    now_ms: i64,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<ContentReference>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let content = prepare(text, row_ids)?;
    insert_prepared(scope, owner_id, &content, text, now_ms).await?;
    Ok(content)
}

pub(crate) fn prepare(
    text: &InstructionText,
    row_ids: &mut FleetRowIds,
) -> AutomationStoreResult<ContentReference> {
    let content_id: ContentId = row_ids
        .next()?
        .to_string()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("content_id"))?;
    let revision_id: RevisionId = row_ids
        .next()?
        .to_string()
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("revision_id"))?;
    let digest = text_digest(text.as_str())?;
    Ok(ContentReference {
        content_id,
        revision_id,
        digest,
    })
}

pub(crate) async fn insert_prepared<Scope: TransactionScope>(
    scope: &mut Scope,
    owner_id: Uuid,
    content: &ContentReference,
    text: &InstructionText,
    now_ms: i64,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    crate::automation_store_error::timestamp(now_ms)?;
    crate::fleet_row_ids::validate_row_id(owner_id)?;
    if content.digest != text_digest(text.as_str())? {
        return Err(AutomationStoreError::InvalidRecord("digest"));
    }
    queries::insert_content_revision(
        scope,
        content.content_id.as_str().into(),
        owner_id.to_string(),
        content.revision_id.as_str().into(),
        now_ms.to_string(),
        content.digest.as_str().into(),
        text.as_str().into(),
    )
    .await?;
    Ok(())
}

pub(crate) async fn read_wake<Scope: TransactionScope>(
    scope: &mut Scope,
    id: &ContentId,
) -> AutomationStoreResult<Option<ContentRevision>>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let Some(row) = queries::read_content_revision(scope, id.as_str().into()).await? else {
        return Ok(None);
    };
    let content = decode(row, LimitName::WakeMessageMaxBytes)?;
    if content.reference.content_id != *id {
        return Err(AutomationStoreError::InvalidRecord("content_id"));
    }
    Ok(Some(content))
}
