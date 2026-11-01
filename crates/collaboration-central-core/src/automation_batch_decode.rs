//! One decoder binds immutable Batch bytes to their digest and destination.
use crate::{automation_content_store::text_digest, automation_store_error::{AutomationStoreError, AutomationStoreResult, required}, generated_automation_custody as queries};
use collaboration_protocol::{AutomationBatch, AutomationRefusal, FrozenAutomationBatch};
use message_board::ServiceId;
pub(crate) struct DecodedAutomationBatch {
    pub frozen: FrozenAutomationBatch,
    pub batch: AutomationBatch,
}

pub(crate) fn decode(
    row: queries::StoredCustodyBatch,
    installation: &ServiceId,
) -> AutomationStoreResult<DecodedAutomationBatch> {
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
    if batch.batch_id != batch_id || String::from(batch.service_id.clone()) != installation.as_str() {
        return Err(AutomationStoreError::InvalidRecord("batch.identity"));
    }
    Ok(DecodedAutomationBatch {
        frozen: FrozenAutomationBatch {
        batch_id,
        digest,
        frozen_bytes: bytes.into_bytes(),
        },
        batch,
    })
}

