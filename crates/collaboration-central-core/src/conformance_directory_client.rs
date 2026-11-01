//! Real directory caller, controlled assertions and a verifier-owned issuer double.
use crate::{
    CallOutcome, ClientAssertionRefusal, DirectoryServiceClient, Ed25519ClientSigner,
    ServiceClientId, ServiceFormTransport, ServiceJsonTransport,
    conformance_client_authentication::controlled_client,
    fleet_directory::OrganizationInitializationPort,
    identity_contract::{
        ApprovalActivation, ApprovalAuthorization, ApprovalCancellation, AuthorizeDeviceApproval,
        CentralRecord, InitializeOrganization, OrganizationInitialization, ResourceUri,
        SettleDeviceApproval,
    },
};
use serde::{Deserialize, Serialize};

pub const DIRECTORY_CLIENT_PROBE_PATH: &str = "/__conformance/directory-client";
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectoryClientProbe {
    now_millis: i64,
    first_id: message_board::RecordId,
    requests: Vec<DirectoryProbeRequest>,
}
#[derive(Deserialize)]
#[serde(
    tag = "operation",
    content = "request",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum DirectoryProbeRequest {
    Authorize(AuthorizeDeviceApproval),
    Activate(SettleDeviceApproval),
    Cancel(SettleDeviceApproval),
    Initialize(InitializeOrganization),
}
#[derive(Serialize)]
#[serde(tag = "result", content = "answer", rename_all = "camelCase")]
pub enum DirectoryProbeAnswer<Reply> {
    Answered(Reply),
    Unknown,
}
impl<Reply> From<CallOutcome<Reply>> for DirectoryProbeAnswer<Reply> {
    fn from(outcome: CallOutcome<Reply>) -> Self {
        match outcome {
            CallOutcome::Answered(answer) => Self::Answered(answer),
            CallOutcome::Failed(_) => Self::Unknown,
        }
    }
}
#[derive(Serialize)]
#[serde(tag = "operation", content = "outcome", rename_all = "camelCase")]
pub enum DirectoryClientObservation {
    Authorize(DirectoryProbeAnswer<ApprovalAuthorization>),
    Activate(DirectoryProbeAnswer<ApprovalActivation>),
    Cancel(DirectoryProbeAnswer<ApprovalCancellation>),
    Initialize(DirectoryProbeAnswer<OrganizationInitialization>),
}
pub async fn observe_directory_client<
    Signer: Ed25519ClientSigner + 'static,
    Transport: ServiceFormTransport + ServiceJsonTransport + 'static,
>(
    signer: Signer,
    transport: Transport,
    client_id: ServiceClientId,
    token_endpoint: ResourceUri,
    central: &CentralRecord,
    input: DirectoryClientProbe,
) -> Result<Vec<DirectoryClientObservation>, ClientAssertionRefusal> {
    let client = DirectoryServiceClient::new(
        controlled_client(signer, client_id, input.now_millis, &input.first_id)?,
        transport,
        token_endpoint,
    );
    let mut outcomes = Vec::with_capacity(input.requests.len());
    for request in input.requests {
        outcomes.push(match request {
            DirectoryProbeRequest::Authorize(body) => DirectoryClientObservation::Authorize(
                client.authorize_approval(central, body).await.into(),
            ),
            DirectoryProbeRequest::Activate(body) => DirectoryClientObservation::Activate(
                client.activate_approval(central, body).await.into(),
            ),
            DirectoryProbeRequest::Cancel(body) => DirectoryClientObservation::Cancel(
                client.cancel_approval(central, body).await.into(),
            ),
            DirectoryProbeRequest::Initialize(body) => DirectoryClientObservation::Initialize(
                client
                    .initialize_organization(
                        central.directory_calls.initialization_address.clone(),
                        body,
                    )
                    .await
                    .into(),
            ),
        });
    }
    Ok(outcomes)
}
