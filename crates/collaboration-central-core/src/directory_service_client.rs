//! The directory sends one credential and one common typed request per step.
use crate::{
    CallOutcome, Ed25519ClientSigner, PrivateKeyJwtClient, ServiceFormTransport,
    ServiceHttpFailure, ServiceOperation,
    fleet_directory::OrganizationInitializationPort,
    identity_contract::{
        ApprovalActivation, ApprovalAuthorization, ApprovalCancellation, AuthorizeDeviceApproval,
        CentralRecord, InitializeOrganization, OrganizationInitialization, ResourceUri,
        SettleDeviceApproval,
    },
};
use durable_steps::{CallFailure, NotSentReason};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use std::future::Future;

/// Bearer bytes are confined to the transport; they never appear in Debug.
pub struct ServiceBearerCredential(String);
impl ServiceBearerCredential {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub trait ServiceJsonTransport: Send + Sync {
    fn post_service_json(
        &self,
        destination: &ResourceUri,
        bearer: &ServiceBearerCredential,
        body: &[u8],
    ) -> impl Future<Output = Result<Vec<u8>, ServiceHttpFailure>> + Send;
}

pub struct DirectoryServiceClient<Signer, Transport> {
    client: PrivateKeyJwtClient<Signer>,
    transport: Transport,
    token_endpoint: ResourceUri,
}
impl<Signer, Transport> DirectoryServiceClient<Signer, Transport> {
    #[must_use]
    pub fn new(
        client: PrivateKeyJwtClient<Signer>,
        transport: Transport,
        token_endpoint: ResourceUri,
    ) -> Self {
        Self {
            client,
            transport,
            token_endpoint,
        }
    }
}
impl<Signer: Ed25519ClientSigner, Transport: ServiceFormTransport + ServiceJsonTransport>
    DirectoryServiceClient<Signer, Transport>
{
    pub async fn authorize_approval(
        &self,
        central: &CentralRecord,
        request: AuthorizeDeviceApproval,
    ) -> CallOutcome<ApprovalAuthorization> {
        self.call(
            &central.directory_calls.approval_authorize_address,
            ServiceOperation::DeviceApprovalAuthorize,
            request,
        )
        .await
    }
    pub async fn activate_approval(
        &self,
        central: &CentralRecord,
        request: SettleDeviceApproval,
    ) -> CallOutcome<ApprovalActivation> {
        self.call(
            &central.directory_calls.approval_activate_address,
            ServiceOperation::DeviceApprovalActivate,
            request,
        )
        .await
    }
    pub async fn cancel_approval(
        &self,
        central: &CentralRecord,
        request: SettleDeviceApproval,
    ) -> CallOutcome<ApprovalCancellation> {
        self.call(
            &central.directory_calls.approval_cancel_address,
            ServiceOperation::DeviceApprovalCancel,
            request,
        )
        .await
    }

    async fn call<Request: Serialize + Send, Reply: DeserializeOwned>(
        &self,
        destination: &ResourceUri,
        operation: ServiceOperation,
        request: Request,
    ) -> CallOutcome<Reply> {
        let Ok(body) = serde_json::to_vec(&request) else {
            return not_sent();
        };
        let Ok(authentication) = self.client.authenticate_at(&self.token_endpoint).await else {
            return not_sent();
        };
        let form = authentication.service_token_form(destination, operation);
        let Ok(credential) = self
            .transport
            .post_service_form(&self.token_endpoint, &form)
            .await
        else {
            return not_sent();
        };
        let Ok(credential) = decode_credential(&credential, operation.scope()) else {
            return not_sent();
        };
        // After this point a failed/unreadable response is unknown. A typed
        // refusal only exists when the central's actual reply decodes as one.
        let Ok(answer) = self
            .transport
            .post_service_json(destination, &credential, &body)
            .await
        else {
            return CallOutcome::Failed(CallFailure::AnswerLost);
        };
        match serde_json::from_slice(&answer) {
            Ok(answer) => CallOutcome::Answered(answer),
            Err(_) => CallOutcome::Failed(CallFailure::AnswerLost),
        }
    }
}
impl<
    Signer: Ed25519ClientSigner + 'static,
    Transport: ServiceFormTransport + ServiceJsonTransport + 'static,
> OrganizationInitializationPort for DirectoryServiceClient<Signer, Transport>
{
    async fn initialize_organization(
        &self,
        destination: ResourceUri,
        request: InitializeOrganization,
    ) -> CallOutcome<OrganizationInitialization> {
        self.call(
            &destination,
            ServiceOperation::InitializeOrganization,
            request,
        )
        .await
    }
}

// Standard provider-owned OAuth response projection, not a K7 JWT schema.
#[derive(Deserialize)]
struct ClientCredentialsReply {
    access_token: String,
    #[serde(rename = "token_type")]
    _token_type: BearerType,
    scope: String,
}
struct BearerType;
impl<'input> Deserialize<'input> for BearerType {
    fn deserialize<Decoder: Deserializer<'input>>(
        decoder: Decoder,
    ) -> Result<Self, Decoder::Error> {
        if !String::deserialize(decoder)?.eq_ignore_ascii_case("Bearer") {
            return Err(serde::de::Error::custom("service credential is not Bearer"));
        }
        Ok(Self)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
enum CredentialRefusal {
    #[error("service credential reply is malformed")]
    Malformed,
    #[error("service credential scope differs from the operation")]
    WrongScope,
    #[error("service credential bytes are invalid")]
    InvalidToken,
}
fn decode_credential(
    bytes: &[u8],
    scope: &str,
) -> Result<ServiceBearerCredential, CredentialRefusal> {
    let reply: ClientCredentialsReply =
        serde_json::from_slice(bytes).map_err(|_| CredentialRefusal::Malformed)?;
    if reply.scope != scope {
        return Err(CredentialRefusal::WrongScope);
    }
    if reply.access_token.is_empty()
        || !reply.access_token.is_ascii()
        || reply
            .access_token
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return Err(CredentialRefusal::InvalidToken);
    }
    Ok(ServiceBearerCredential(reply.access_token))
}
fn not_sent<Reply>() -> CallOutcome<Reply> {
    CallOutcome::Failed(CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    })
}
