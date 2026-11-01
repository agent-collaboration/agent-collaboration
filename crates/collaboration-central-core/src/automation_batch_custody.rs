//! A machine reads exactly its committed Batch bytes until its durable acknowledgement.
use crate::{
    automation_content_store::text_digest,
    automation_store_error::{AutomationStoreError, AutomationStoreResult, required},
    generated_automation_custody as queries,
};
use collaboration_protocol::{
    AutomationAnswer, AutomationBatch, AutomationBatches, AutomationRefusal,
    AutomationRequestConstraint, FrozenAutomationBatch, PullAutomationBatches,
};
use durable_steps::storage::TransactionScope;
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

fn decode(
    row: queries::StoredCustodyBatch,
    installation: &ServiceId,
) -> AutomationStoreResult<FrozenAutomationBatch> {
    let service: ServiceId = required(row.service_id, "batch.service_id")?
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("batch.service_id"))?;
    if service != *installation {
        return Err(AutomationStoreError::Refused(
            AutomationRefusal::OriginMismatch,
        ));
    }
    let batch_id = required(row.batch_id, "batch_id")?
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("batch_id"))?;
    let digest = required(row.digest, "batch.digest")?
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("batch.digest"))?;
    let bytes = required(row.frozen_bytes, "batch.frozen_bytes")?;
    if text_digest(&bytes)? != digest {
        return Err(AutomationStoreError::InvalidRecord("batch.digest"));
    }
    let raw: serde_json::Value = serde_json::from_str(&bytes)
        .map_err(|_| AutomationStoreError::InvalidRecord("batch.frozen_bytes"))?;
    crate::automation_request_decode::identity_limits(&raw)?;
    crate::automation_request_decode::counter_limits(&raw)?;
    let batch: AutomationBatch = serde_json::from_value(raw)
        .map_err(|_| AutomationStoreError::InvalidRecord("batch.frozen_bytes"))?;
    if batch.batch_id != batch_id || String::from(batch.service_id) != installation.as_str() {
        return Err(AutomationStoreError::InvalidRecord("batch.identity"));
    }
    Ok(FrozenAutomationBatch {
        batch_id,
        digest,
        frozen_bytes: bytes.into_bytes(),
    })
}

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
        decode(row, installation)?;
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
        let batch = decode(row, installation)?;
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
    for ack in &request.batches {
        let row = queries::read_automation_batch(scope, ack.batch_id.as_str().into())
            .await?
            .ok_or(AutomationStoreError::Refused(AutomationRefusal::NotFound))?;
        let batch = decode(row, installation)?;
        if batch.digest != ack.digest {
            return Err(AutomationStoreError::Refused(
                AutomationRefusal::DigestMismatch,
            ));
        }
    }
    for ack in &request.batches {
        queries::ack_automation_batch(scope, now_ms.to_string(), ack.batch_id.as_str().into())
            .await?;
        queries::receive_automation_firings(scope, ack.batch_id.as_str().into()).await?;
    }
    Ok(AutomationAnswer::Acknowledged)
}
