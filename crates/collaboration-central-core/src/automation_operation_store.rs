//! Operation IDs retain their original catalog answer in the mutation transaction.
use crate::{
    automation_store_error::{AutomationStoreError, AutomationStoreResult, required},
    generated_automation_operation as queries,
};
use collaboration_mail_protocol::MailDigest;
use collaboration_protocol::{AutomationAnswer, OperationId};
use durable_steps::storage::TransactionScope;
use sha2::{Digest, Sha256};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) fn request_digest(request: &impl serde::Serialize) -> AutomationStoreResult<MailDigest> {
    let bytes =
        serde_json::to_vec(request).map_err(|_| AutomationStoreError::InvalidRecord("request"))?;
    format!("{:x}", Sha256::digest(bytes))
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("digest"))
}

pub(crate) async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    operation_id: &OperationId,
) -> AutomationStoreResult<Option<(MailDigest, AutomationAnswer)>>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let Some(row) = queries::read_automation_operation(scope, operation_id.as_str().into()).await?
    else {
        return Ok(None);
    };
    let digest = required(row.digest, "automation_operations.digest")?
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("automation_operations.digest"))?;
    let value: serde_json::Value =
        serde_json::from_str(&required(row.answer, "automation_operations.answer")?)
            .map_err(|_| AutomationStoreError::InvalidRecord("automation_operations.answer"))?;
    crate::automation_request_decode::answer_limits(&value)?;
    let answer = serde_json::from_value(value)
        .map_err(|_| AutomationStoreError::InvalidRecord("automation_operations.answer"))?;
    Ok(Some((digest, answer)))
}

pub(crate) async fn record<Scope: TransactionScope>(
    scope: &mut Scope,
    operation_id: &OperationId,
    digest: &MailDigest,
    answer: &AutomationAnswer,
) -> AutomationStoreResult<()>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    let answer =
        serde_json::to_string(answer).map_err(|_| AutomationStoreError::InvalidRecord("answer"))?;
    queries::insert_automation_operation(
        scope,
        operation_id.as_str().into(),
        digest.as_str().into(),
        answer,
    )
    .await?;
    Ok(())
}
