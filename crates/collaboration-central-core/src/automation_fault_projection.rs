//! Fault inspection depends on row identity and the recorded refusal, never executable JSON.
use crate::{
    automation_store_error::{
        AutomationStoreError, AutomationStoreResult, required, timestamp, timestamp_millis,
    },
    generated_automation_catalog::StoredAutomation,
};
use collaboration_protocol::{
    AutomationFault, AutomationKind, AutomationRefusal, FaultedAutomationSnapshot,
};

pub(crate) fn projection(
    row: &StoredAutomation,
) -> AutomationStoreResult<Option<FaultedAutomationSnapshot>> {
    let Some(json) = row.fault_json.as_ref() else {
        if row.faulted_at_ms.is_some() {
            return Err(AutomationStoreError::InvalidRecord("fault_json"));
        }
        return Ok(None);
    };
    let raw: serde_json::Value = serde_json::from_str(json)
        .map_err(|_| AutomationStoreError::InvalidRecord("fault_json"))?;
    crate::automation_request_decode::identity_limits(&raw)?;
    crate::automation_request_decode::counter_limits(&raw)?;
    let error: AutomationRefusal = serde_json::from_value(raw)
        .map_err(|_| AutomationStoreError::InvalidRecord("fault_json"))?;
    let kind = match row.kind.as_deref() {
        Some("schedule") => AutomationKind::Schedule,
        Some("wake") => AutomationKind::Wake,
        _ => return Err(AutomationStoreError::InvalidRecord("kind")),
    };
    Ok(Some(FaultedAutomationSnapshot {
        automation_id: required(row.automation_id.clone(), "automation_id")?
            .try_into()
            .map_err(|_| AutomationStoreError::InvalidRecord("automation_id"))?,
        revision: required(row.revision.clone(), "revision")?
            .try_into()
            .map_err(|_| AutomationStoreError::InvalidRecord("revision"))?,
        kind,
        fault: AutomationFault {
            error,
            faulted_at: timestamp(timestamp_millis(
                row.faulted_at_ms.clone(),
                "faulted_at_ms",
            )?)?,
        },
    }))
}
