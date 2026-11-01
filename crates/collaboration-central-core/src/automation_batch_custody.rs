//! A machine reads exactly its committed Batch bytes until its durable acknowledgement.
use crate::{
    automation_batch_decode::decode,
    machine_work_item_store,
    automation_store_error::{AutomationStoreError, AutomationStoreResult},
    generated_automation_custody as queries,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationBatches, AutomationRefusal,
    AutomationRequestConstraint, MachineWorkCustodyState, PullAutomationBatches,
};
use durable_steps::storage::TransactionScope;
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn pull<Scope: TransactionScope>(
    scope: &mut Scope,
    installation: &ServiceId,
    request: &PullAutomationBatches,
) -> AutomationStoreResult<AutomationAnswer>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    if let Some(after) = &request.after {
        let row = queries::read_automation_batch(scope, after.as_str().into())
            .await?
            .ok_or(AutomationStoreError::InvalidRequest {
                field: "after",
                reason: AutomationRequestConstraint::InvalidCursor,
            })?;
        let decoded = decode(row, installation)?;
        machine_work_item_store::validate_batch(scope, &decoded.batch).await?;
    }
    let rows = queries::list_automation_batches(
        scope,
        installation.as_str().into(),
        request
            .after
            .as_ref()
            .map(|id| id.as_str().into())
            .unwrap_or_default(),
    )
    .await?;
    let budget = request.max_bytes as usize;
    let mut page = AutomationBatches {
        batches: Vec::new(),
        next: None,
    };
    for row in rows {
        let decoded = decode(row, installation)?;
        machine_work_item_store::validate_batch(scope, &decoded.batch).await?;
        let batch = decoded.frozen;
        // Budget the typed wire answer, including JSON's byte-array expansion
        // and a continuation, rather than only counting raw frozen bytes.
        let previous = page.next.clone();
        page.next = Some(batch.batch_id.clone());
        page.batches.push(batch);
        let size = serde_json::to_vec(&AutomationAnswer::Batches(page.clone()))
            .map_err(|_| AutomationStoreError::InvalidRecord("batch.page"))?
            .len();
        if size > budget {
            page.batches.pop();
            page.next = previous;
            if page.batches.is_empty() {
                return Err(AutomationStoreError::InvalidRequest {
                    field: "maxBytes",
                    reason: AutomationRequestConstraint::MalformedShape,
                });
            }
            return Ok(AutomationAnswer::Batches(page));
        }
    }
    page.next = None;
    Ok(AutomationAnswer::Batches(page))
}

pub(crate) async fn acknowledge<Scope: TransactionScope>(
    scope: &mut Scope,
    installation: &ServiceId,
    request: &collaboration_protocol::AckAutomationBatches,
    now_ms: i64,
) -> AutomationStoreResult<AutomationAnswer>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut validated = Vec::with_capacity(request.batches.len());
    for ack in &request.batches {
        let row = queries::read_automation_batch(scope, ack.batch_id.as_str().into())
            .await?
            .ok_or(AutomationStoreError::Refused(AutomationRefusal::NotFound))?;
        let decoded = decode(row, installation)?;
        machine_work_item_store::validate_batch(scope, &decoded.batch).await?;
        if decoded.frozen.digest != ack.digest {
            return Err(AutomationStoreError::Refused(
                AutomationRefusal::DigestMismatch,
            ));
        }
        validated.push(decoded.batch);
    }
    for (ack, batch) in request.batches.iter().zip(&validated) {
        queries::ack_automation_batch(scope, now_ms.to_string(), ack.batch_id.as_str().into())
            .await?;
        queries::receive_automation_firings(scope, ack.batch_id.as_str().into()).await?;
        machine_work_item_store::advance_batch_custody(scope, batch, MachineWorkCustodyState::Received).await?;
    }
    Ok(AutomationAnswer::Acknowledged)
}
