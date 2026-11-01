//! Raw owner observations are setup and evidence; product calls remain the oracle.
use crate::{ConformanceClient, ConformanceResult};
use collaboration_central_core::{MachineWorkFixtureMutation, StepControl, StepControlReply};
use collaboration_protocol::WorkItemId;
use serde::de::DeserializeOwned;

pub(super) async fn machine_work(
    client: &ConformanceClient,
    work_item: &WorkItemId,
    mutation: Option<MachineWorkFixtureMutation>,
) -> ConformanceResult<serde_json::Value> {
    match owner_step(
        client,
        StepControl::MachineWorkFixture {
            work_item_id: work_item.clone(),
            mutation,
        },
    )
    .await?
    {
        StepControlReply::MachineWorkFixture(raw) => Ok(raw),
        other => Err(format!("unexpected machine-work fixture reply: {other:?}").into()),
    }
}

pub(super) async fn automation(
    client: &ConformanceClient,
    id: &collaboration_protocol::AutomationId,
) -> ConformanceResult<serde_json::Value> {
    match owner_step(
        client,
        StepControl::AutomationFixture {
            automation_id: id.clone(),
            definition_json: None,
            mutation: None,
        },
    )
    .await?
    {
        StepControlReply::AutomationFixture(raw) => Ok(raw),
        other => Err(format!("unexpected Automation fixture reply: {other:?}").into()),
    }
}

async fn owner_step(
    client: &ConformanceClient,
    command: StepControl,
) -> ConformanceResult<StepControlReply> {
    let response = client
        .with_owner_access(client.client.post(format!(
            "{}/admin/v1/conformance/fleet-receipt",
            client.base_url
        )))?
        .json(&command)
        .send()
        .await?
        .error_for_status()?;
    let reply: Result<StepControlReply, durable_steps::StoreError> = response.json().await?;
    Ok(reply?)
}

pub(super) fn field<T: DeserializeOwned>(
    raw: &serde_json::Value,
    pointer: &str,
) -> ConformanceResult<T> {
    Ok(serde_json::from_value(
        raw.pointer(pointer)
            .ok_or("raw machine-work field missing")?
            .clone(),
    )?)
}
