//! The directory's exact C7 requests; issuer/HTTP answers are explicit stand-ins.
#![cfg(test)]
use collaboration_central_core::{
    CallOutcome, ClientAssertionLifetime, ClientAssertionRefusal, DirectoryServiceClient,
    Ed25519ClientSigner, PrivateKeyJwtClient, ServiceBearerCredential, ServiceClientId,
    ServiceForm, ServiceFormTransport, ServiceHttpFailure, ServiceJsonTransport,
    fleet_directory::OrganizationInitializationPort,
    identity_contract::{
        ApprovalActivation, ApprovalAuthorization, ApprovalCancellation, AuthorizeDeviceApproval,
        CentralRecord, InitializeOrganization, OrganizationInitialization, ResourceUri,
        SettleDeviceApproval,
    },
};
use runtime_primitives::{Clock, IdSource, SyncTime};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(2_208_988_800_000)
    }
}
#[derive(Default)]
struct ScriptedIds(AtomicU64);
impl IdSource for ScriptedIds {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::from_u128(
            0x019f0000000070008000000000000000 + u128::from(self.0.fetch_add(1, Ordering::SeqCst)),
        )
    }
}
struct SignerDouble;
impl Ed25519ClientSigner for SignerDouble {
    fn key_id(&self) -> &str {
        "public-fixture-key"
    }
    async fn sign_client_assertion(&self, _: &[u8]) -> Result<Vec<u8>, ClientAssertionRefusal> {
        Ok(vec![1; 64])
    }
}
enum RequestObservation {
    Form {
        uri: String,
        fields: BTreeMap<String, String>,
    },
    Json {
        uri: String,
        bearer: String,
        body: Value,
    },
}
#[derive(Clone)]
struct ScriptedTransport {
    state: Arc<Mutex<ScriptedTransportState>>,
}
struct ScriptedTransportState {
    replies: VecDeque<Result<Vec<u8>, ServiceHttpFailure>>,
    requests: Vec<RequestObservation>,
    unexpected_calls: usize,
}
impl ScriptedTransport {
    fn new(replies: Vec<Result<Vec<u8>, ServiceHttpFailure>>) -> Self {
        Self {
            state: Arc::new(Mutex::new(ScriptedTransportState {
                replies: replies.into(),
                requests: Vec::new(),
                unexpected_calls: 0,
            })),
        }
    }
    fn answer(&self) -> Result<Vec<u8>, ServiceHttpFailure> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ServiceHttpFailure::Unreachable)?;
        match state.replies.pop_front() {
            Some(answer) => answer,
            None => {
                state.unexpected_calls += 1;
                Err(ServiceHttpFailure::Unreachable)
            }
        }
    }
    fn record(&self, request: RequestObservation) {
        self.state.lock().unwrap().requests.push(request);
    }
    fn assert_finished(&self, expected_requests: usize) {
        let state = self.state.lock().unwrap();
        assert_eq!(state.requests.len(), expected_requests);
        assert!(state.replies.is_empty(), "scripted replies were unused");
        assert_eq!(
            state.unexpected_calls, 0,
            "unexpected service request exhausted the script"
        );
    }
}
impl ServiceFormTransport for ScriptedTransport {
    async fn post_service_form(
        &self,
        endpoint: &ResourceUri,
        form: &ServiceForm,
    ) -> Result<Vec<u8>, ServiceHttpFailure> {
        self.record(RequestObservation::Form {
            uri: endpoint.as_str().into(),
            fields: url::form_urlencoded::parse(form.as_str().as_bytes())
                .into_owned()
                .collect(),
        });
        self.answer()
    }
}
impl ServiceJsonTransport for ScriptedTransport {
    async fn post_service_json(
        &self,
        destination: &ResourceUri,
        bearer: &ServiceBearerCredential,
        body: &[u8],
    ) -> Result<Vec<u8>, ServiceHttpFailure> {
        self.record(RequestObservation::Json {
            uri: destination.as_str().into(),
            bearer: bearer.as_str().into(),
            body: serde_json::from_slice(body).unwrap(),
        });
        self.answer()
    }
}
fn uri(value: &str) -> ResourceUri {
    ResourceUri::try_from(value.to_owned()).unwrap()
}
fn client(transport: ScriptedTransport) -> DirectoryServiceClient<SignerDouble, ScriptedTransport> {
    DirectoryServiceClient::new(
        PrivateKeyJwtClient::new(
            ServiceClientId::try_from("pinned-directory".to_owned()).unwrap(),
            SignerDouble,
            Arc::new(FixedClock),
            Arc::new(ScriptedIds::default()),
            ClientAssertionLifetime::from_seconds(60).unwrap(),
        )
        .unwrap(),
        transport,
        uri("https://issuer.example.invalid/api/auth/oauth2/token"),
    )
}
fn central() -> CentralRecord {
    serde_json::from_value(json!({"addresses":{
        "sessionEndpoint":"https://central.example.invalid/v1/session","machineAddress":"https://central.example.invalid/machine",
        "adminAddress":"https://central.example.invalid/admin","admissionAddress":"https://central.example.invalid/admission",
        "standingAddress":"https://central.example.invalid/standing","machineTokenAddress":"https://central.example.invalid/machine-token",
        "receiver":"019f0000-0000-7000-8000-000000000002","relay":"019f0000-0000-7000-8000-000000000003"},
        "directoryCalls":{"approvalAuthorizeAddress":"https://central.example.invalid/authorize","approvalActivateAddress":"https://central.example.invalid/activate",
            "approvalCancelAddress":"https://central.example.invalid/cancel","initializationAddress":"https://central.example.invalid/initialize"}})).unwrap()
}
fn decision() -> Value {
    json!({"decision":{"organizationId":"019f0000-0000-7000-8000-000000000001",
    "approver":"019f0000-0000-7000-8000-000000000002","requestId":"019f0000-0000-7000-8000-000000000003",
    "installation":"019f0000-0000-7000-8000-000000000004","peer":"019f0000-0000-7000-8000-000000000005",
    "label":"fixture machine","allowedWorkspaces":[]}})
}
fn settlement() -> Value {
    json!({"organizationId":"019f0000-0000-7000-8000-000000000001","requestId":"019f0000-0000-7000-8000-000000000003"})
}
fn initialization() -> Value {
    json!({"organizationId":"019f0000-0000-7000-8000-000000000001","ownerEmail":"owner@example.invalid","commandId":"019f0000-0000-7000-8000-000000000006"})
}
fn reply(value: Value) -> Result<Vec<u8>, ServiceHttpFailure> {
    serde_json::to_vec(&value).map_err(|_| ServiceHttpFailure::InvalidReply)
}
fn credential(scope: &str) -> Result<Vec<u8>, ServiceHttpFailure> {
    reply(
        json!({"access_token":"public.fixture-service-token","token_type":"Bearer","scope":scope,"expires_in":300}),
    )
}

// Catches scope/address sharing or serializing a step as another body. The
// common DTOs and caller run; real issuer/K7 validation remains unverified.
#[tokio::test(start_paused = true)]
async fn every_directory_step_uses_its_record_address_scope_and_common_body() {
    let transport = ScriptedTransport::new(vec![
        credential("device-approval-authorize"),
        reply(json!({"result":"reserved"})),
        credential("device-approval-activate"),
        reply(json!({"result":"activated","enrollmentId":"019f0000-0000-7000-8000-000000000007"})),
        credential("device-approval-cancel"),
        reply(json!({"result":"cancelled"})),
        credential("initialize-organization"),
        reply(json!({"result":"alreadyInitialized"})),
    ]);
    let client = client(transport.clone());
    let central = central();
    assert!(matches!(
        client
            .authorize_approval(
                &central,
                serde_json::from_value::<AuthorizeDeviceApproval>(decision()).unwrap()
            )
            .await,
        CallOutcome::Answered(ApprovalAuthorization::Reserved {})
    ));
    assert!(matches!(
        client
            .activate_approval(
                &central,
                serde_json::from_value::<SettleDeviceApproval>(settlement()).unwrap()
            )
            .await,
        CallOutcome::Answered(ApprovalActivation::Activated { .. })
    ));
    assert!(matches!(
        client
            .cancel_approval(
                &central,
                serde_json::from_value::<SettleDeviceApproval>(settlement()).unwrap()
            )
            .await,
        CallOutcome::Answered(ApprovalCancellation::Cancelled {})
    ));
    assert!(matches!(
        client
            .initialize_organization(
                central.directory_calls.initialization_address.clone(),
                serde_json::from_value::<InitializeOrganization>(initialization()).unwrap()
            )
            .await,
        CallOutcome::Answered(OrganizationInitialization::AlreadyInitialized {})
    ));
    transport.assert_finished(8);
    let state = transport.state.lock().unwrap();
    let (pairs, remainder) = state.requests.as_chunks::<2>();
    assert!(remainder.is_empty());
    for (pair, (path, scope, body)) in pairs.iter().zip([
        ("authorize", "device-approval-authorize", decision()),
        ("activate", "device-approval-activate", settlement()),
        ("cancel", "device-approval-cancel", settlement()),
        ("initialize", "initialize-organization", initialization()),
    ]) {
        let destination = format!("https://central.example.invalid/{path}");
        match &pair[0] {
            RequestObservation::Form { uri, fields } => {
                assert_eq!(uri, "https://issuer.example.invalid/api/auth/oauth2/token");
                assert_eq!(fields.len(), 6);
                assert_eq!(fields["grant_type"], "client_credentials");
                assert_eq!(fields["resource"], destination);
                assert_eq!(fields["scope"], scope);
                assert_eq!(fields["client_id"], "pinned-directory");
                assert_eq!(
                    fields["client_assertion_type"],
                    "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"
                );
            }
            RequestObservation::Json { .. } => panic!("operation ran before authentication"),
        }
        match &pair[1] {
            RequestObservation::Json {
                uri,
                bearer,
                body: sent,
            } => {
                assert_eq!(uri, &destination);
                assert_eq!(bearer, "public.fixture-service-token");
                assert_eq!(sent, &body);
            }
            RequestObservation::Form { .. } => panic!("operation body was not sent"),
        }
    }
}

#[tokio::test(start_paused = true)]
async fn a_failed_or_wrong_scope_credential_sends_no_operation() {
    for answer in [
        Err(ServiceHttpFailure::Unreachable),
        reply(json!({})),
        credential("device-approval-cancel"),
        reply(
            json!({"access_token":"public.fixture-service-token","token_type":"DPoP","scope":"initialize-organization"}),
        ),
        reply(
            json!({"access_token":"bad\r\ncredential","token_type":"Bearer","scope":"initialize-organization"}),
        ),
    ] {
        let transport = ScriptedTransport::new(vec![answer]);
        let client = client(transport.clone());
        let answer = client
            .initialize_organization(
                central().directory_calls.initialization_address,
                serde_json::from_value(initialization()).unwrap(),
            )
            .await;
        assert!(matches!(
            answer,
            CallOutcome::Failed(durable_steps::CallFailure::NotSent { .. })
        ));
        transport.assert_finished(1);
    }
}

#[tokio::test(start_paused = true)]
async fn an_unknown_step_answer_is_never_guessed_as_a_refusal_or_success() {
    for answer in [
        Err(ServiceHttpFailure::Unreachable),
        reply(json!({"result":"unknown"})),
        Ok(b"not-json".to_vec()),
    ] {
        let transport =
            ScriptedTransport::new(vec![credential("device-approval-authorize"), answer]);
        let client = client(transport.clone());
        let answer = client
            .authorize_approval(&central(), serde_json::from_value(decision()).unwrap())
            .await;
        assert!(matches!(
            answer,
            CallOutcome::Failed(durable_steps::CallFailure::AnswerLost)
        ));
        transport.assert_finished(2);
    }
    let transport = ScriptedTransport::new(vec![
        credential("device-approval-authorize"),
        reply(json!({"result":"refused","reason":{"kind":"notOwner"}})),
    ]);
    assert!(matches!(
        client(transport.clone())
            .authorize_approval(&central(), serde_json::from_value(decision()).unwrap())
            .await,
        CallOutcome::Answered(ApprovalAuthorization::Refused { .. })
    ));
    transport.assert_finished(2);
}
