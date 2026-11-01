//! One retained request binds the item, operation, source and reporting target.
use crate::{
    automation_operation_store,
    automation_store_error::{AutomationStoreError, AutomationStoreResult, required},
    generated_machine_work_item::StoredMachineWorkItem,
};
use collaboration_mail_protocol::MailDigest;
use collaboration_protocol::{
    FirstInputState, MachineWorkCustodyState, ProviderIdentity, RemoteSessionCreateRequest,
    RemoteSessionCreateSnapshot, SessionCreateEntry, SessionCreateState, WorkReportSequence,
};
use message_board::ServiceId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum MachineWorkKind {
    SessionCreate,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MachineWorkResult {
    pub seq: WorkReportSequence,
    pub create: SessionCreateState,
    pub input: FirstInputState,
}

pub(crate) struct MachineWorkItemRecord {
    pub entry: SessionCreateEntry,
    pub target: ServiceId,
    pub state: MachineWorkCustodyState,
    pub result: Option<MachineWorkResult>,
}

impl MachineWorkItemRecord {
    pub fn snapshot(&self) -> RemoteSessionCreateSnapshot {
        RemoteSessionCreateSnapshot {
            work_item_id: self.entry.work_item_id.clone(),
            operation_id: self.entry.operation_id.clone(),
            state: self.state,
            create: self.result.as_ref().map(|result| result.create.clone()),
            input: match &self.result {
                Some(result) => Some(result.input.clone()),
                None => self
                    .entry
                    .request
                    .first_input
                    .is_none()
                    .then_some(FirstInputState::None),
            },
            report_seq: self.result.as_ref().map(|result| result.seq),
        }
    }
}

pub(crate) fn request_digest(
    source: &ServiceId,
    request: &RemoteSessionCreateRequest,
) -> AutomationStoreResult<MailDigest> {
    automation_operation_store::request_digest(&(source, request))
}

pub(crate) fn require_creator(
    source: &ServiceId,
    identity: &ProviderIdentity,
) -> AutomationStoreResult<()> {
    if let ProviderIdentity::Session(session) = identity
        && String::from(session.endpoint.service_id.clone()) != source.as_str()
    {
        return Err(AutomationStoreError::Refused(
            collaboration_protocol::AutomationRefusal::OriginMismatch,
        ));
    }
    // A Human is declared; its installation is the authenticated source, not
    // an installation inferred from the opaque human ID.
    Ok(())
}

pub(crate) fn decode(row: StoredMachineWorkItem) -> AutomationStoreResult<MachineWorkItemRecord> {
    let invalid = || AutomationStoreError::InvalidRecord("machine_work_items.request_json");
    let _: MachineWorkKind = serde_json::from_value(serde_json::Value::String(required(
        row.kind,
        "machine_work_items.kind",
    )?))
    .map_err(|_| AutomationStoreError::InvalidRecord("machine_work_items.kind"))?;
    let raw: serde_json::Value = serde_json::from_str(&required(
        row.request_json,
        "machine_work_items.request_json",
    )?)
    .map_err(|_| invalid())?;
    crate::automation_request_decode::identity_limits(&raw)?;
    let entry: SessionCreateEntry = serde_json::from_value(raw).map_err(|_| invalid())?;
    let target: ServiceId = required(row.service_id, "machine_work_items.service_id")?
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("machine_work_items.service_id"))?;
    if required(row.work_item_id, "machine_work_items.work_item_id")? != entry.work_item_id.as_str()
        || required(row.operation_id, "machine_work_items.operation_id")?
            != entry.operation_id.as_str()
        || target.as_str() != String::from(entry.request.endpoint.service_id.clone())
    {
        return Err(invalid());
    }
    require_creator(&entry.source_installation, &entry.request.created_by)
        .map_err(|_| invalid())?;
    let request = RemoteSessionCreateRequest {
        operation_id: entry.operation_id.clone(),
        definition: entry.request.clone(),
    };
    crate::automation_request_decode::call_limits(
        &serde_json::to_value(collaboration_protocol::AutomationCall::RemoteSessionCreate(
            request.clone(),
        ))
        .map_err(|_| invalid())?,
    )?;
    let digest: MailDigest = required(row.request_digest, "machine_work_items.request_digest")?
        .try_into()
        .map_err(|_| AutomationStoreError::InvalidRecord("machine_work_items.request_digest"))?;
    if digest != request_digest(&entry.source_installation, &request)? {
        return Err(AutomationStoreError::InvalidRecord(
            "machine_work_items.request_digest",
        ));
    }
    let state = serde_json::from_value(serde_json::Value::String(required(
        row.state,
        "machine_work_items.state",
    )?))
    .map_err(|_| AutomationStoreError::InvalidRecord("machine_work_items.state"))?;
    let result: Option<MachineWorkResult> = row
        .result_json
        .map(|value| {
            serde_json::from_str(&value)
                .map_err(|_| AutomationStoreError::InvalidRecord("machine_work_items.result_json"))
        })
        .transpose()?;
    if let Some(MachineWorkResult {
        create: SessionCreateState::Created { session_ref, .. },
        ..
    }) = &result
        && String::from(session_ref.endpoint.service_id.clone()) != target.as_str()
    {
        return Err(AutomationStoreError::InvalidRecord(
            "machine_work_items.result_json",
        ));
    }
    Ok(MachineWorkItemRecord {
        entry,
        target,
        state,
        result,
    })
}

#[cfg(test)]
#[path = "machine_work_item_record_tests.rs"]
mod tests;
