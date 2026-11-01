//! The organization list pages central definitions, with all record decoding intact.
use crate::{
    automation_catalog_record::StoredAutomationDefinition,
    automation_catalog_store as catalog,
    automation_store_error::{AutomationStoreError, AutomationStoreResult},
    generated_automation_catalog as queries,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationId, AutomationPageRequest, AutomationRequestConstraint,
    CatalogAutomation,
};
use durable_steps::storage::TransactionScope;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

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
        .as_ref()
        .map(|cursor| AutomationId::try_from(cursor.clone()))
        .transpose()
        .map_err(|_| AutomationStoreError::InvalidRequest {
            field: "cursor",
            reason: AutomationRequestConstraint::InvalidCursor,
        })?;
    let limit = usize::try_from(u32::from(request.limit))
        .map_err(|_| AutomationStoreError::InvalidRecord("page_limit"))?;
    let rows = queries::list_automations(
        scope,
        after.map(String::from).unwrap_or_default(),
        (limit + 1).to_string(),
    )
    .await?;
    let more = rows.len() > limit;
    let mut automations = Vec::new();
    let mut last = None;
    for row in rows.into_iter().take(limit) {
        if let Some(faulted) = crate::automation_fault_projection::projection(&row)? {
            last = Some(faulted.automation_id.as_str().into());
            automations.push(CatalogAutomation::Faulted(faulted));
            continue;
        }
        let record = catalog::decode(row)?;
        let snapshot = match &record.definition {
            StoredAutomationDefinition::Schedule(_) => {
                CatalogAutomation::Schedule(crate::automation_schedule_store::snapshot(&record)?)
            }
            StoredAutomationDefinition::Wake(_) => CatalogAutomation::Wake(
                crate::automation_wake_store::snapshot(scope, &record).await?,
            ),
        };
        last = Some(record.id.as_str().to_owned());
        automations.push(snapshot);
    }
    let next = if more {
        Some(last.ok_or(AutomationStoreError::InvalidRecord("automation_page"))?)
    } else {
        None
    };
    Ok(AutomationAnswer::List { automations, next })
}
