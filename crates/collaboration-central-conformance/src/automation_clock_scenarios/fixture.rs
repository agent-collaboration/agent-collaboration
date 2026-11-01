//! Owner fixture controls alter only the named Automation; normal calls observe behavior.
use crate::{ConformanceClient, ConformanceResult};
use collaboration_central_core::{AutomationFixtureMutation, StepControl, StepControlReply};
use collaboration_protocol::AutomationId;

pub(super) async fn automation(
    client: &ConformanceClient,
    id: &AutomationId,
    definition_json: Option<String>,
) -> ConformanceResult<serde_json::Value> {
    request(client, id, definition_json, None).await
}

pub(super) async fn mutate(
    client: &ConformanceClient,
    id: &AutomationId,
    mutation: AutomationFixtureMutation,
) -> ConformanceResult<serde_json::Value> {
    request(client, id, None, Some(mutation)).await
}

async fn request(
    client: &ConformanceClient,
    id: &AutomationId,
    definition_json: Option<String>,
    mutation: Option<AutomationFixtureMutation>,
) -> ConformanceResult<serde_json::Value> {
    let response = client
        .with_owner_access(client.client.post(format!(
            "{}/admin/v1/conformance/fleet-receipt",
            client.base_url
        )))?
        .json(&StepControl::AutomationFixture {
            automation_id: id.clone(),
            definition_json,
            mutation,
        })
        .send()
        .await?
        .error_for_status()?;
    let reply: Result<StepControlReply, durable_steps::StoreError> = response.json().await?;
    match reply? {
        StepControlReply::AutomationFixture(observation) => Ok(observation),
        other => Err(format!("unexpected Automation fixture reply: {other:?}").into()),
    }
}
