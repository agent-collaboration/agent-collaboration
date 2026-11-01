//! Immutable content is served only to a machine whose Batch references it.
use crate::{
    automation_content_store as contents,
    automation_store_error::{AutomationStoreError, AutomationStoreResult, required},
    generated_automation_custody as queries,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationContent, AutomationRefusal, FetchContents,
};
use durable_steps::storage::TransactionScope;
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn fetch<Scope: TransactionScope>(
    scope: &mut Scope,
    installation: &ServiceId,
    request: &FetchContents,
) -> AutomationStoreResult<AutomationAnswer>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut result = Vec::new();
    for reference in &request.contents {
        let authorized = queries::authorized_automation_content(
            scope,
            reference.content_id.as_str().into(),
            installation.as_str().into(),
        )
        .await?
        .ok_or(AutomationStoreError::Refused(AutomationRefusal::NotFound))?;
        if required(authorized.content_id, "content_id")? != reference.content_id.as_str() {
            return Err(AutomationStoreError::InvalidRecord("content_id"));
        }
        let content = contents::read(scope, &reference.content_id)
            .await?
            .ok_or(AutomationStoreError::InvalidRecord("content_id"))?;
        if content.reference != *reference {
            return Err(AutomationStoreError::Refused(
                AutomationRefusal::DigestMismatch,
            ));
        }
        result.push(AutomationContent {
            reference: content.reference,
            text: content.text,
        });
    }
    Ok(AutomationAnswer::Contents(result))
}
