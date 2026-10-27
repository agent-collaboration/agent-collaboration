#![expect(
    clippy::expect_used,
    reason = "A malformed fixed fixture must fail its test."
)]
//! Provider route decisions before any client effect.

use agent_automation::RouteEffectEvidence;
use codex_router_host::{
    ExternalProviderLaunch, LiveSessionOwnership, LiveSessionOwnershipCheck,
    ProviderAcpDeliveryRoute,
};
use collaboration_contracts::{
    AttemptEvidenceSink, AttemptReconciliation, AttemptReconciliationContext, DeliveryFuture,
    DeliveryPrecondition, LoadPolicy, RoutePresence, SessionDeliveryRoute,
    layer_zero::{DeliveryRequest as PreparedDeliveryRequest, PreparedPush},
};
use collaboration_protocol::{
    AttemptId, ChannelDescription, CodexGeneration, DeliveryCorrelationId, EndpointAvailability,
    EndpointDescription, EndpointId, EndpointRef, GenerationNumber, MessageDelivery, MessageText,
    NonEmptyText, ObservationTimestamp, ProviderBindingId, ProviderBindingIdentity,
    ProviderCapabilities, ProviderCapability, ProviderCapabilityEvidence, ProviderCapabilityName,
    ProviderCapabilityStatus, ProviderKind, ProviderRuntimeIdentity, ProviderTransport, PushId,
    SessionId, SessionRef, UuidIdentity,
};
use collaboration_runtime::EndpointDirectory;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(super) struct NoLivePeer;

pub(super) struct LivePeer;

impl LiveSessionOwnershipCheck for NoLivePeer {
    fn check<'a>(&'a self, _target: &'a SessionRef) -> DeliveryFuture<'a, LiveSessionOwnership> {
        Box::pin(async { Ok(LiveSessionOwnership::NotLive) })
    }
}

impl LiveSessionOwnershipCheck for LivePeer {
    fn check<'a>(&'a self, _target: &'a SessionRef) -> DeliveryFuture<'a, LiveSessionOwnership> {
        Box::pin(async { Ok(LiveSessionOwnership::LiveUnsupported) })
    }
}

pub(super) struct RecordedEvidence(
    pub(super) tokio::sync::Mutex<Vec<RouteEffectEvidence<SessionRef, CodexGeneration>>>,
);

impl AttemptEvidenceSink for RecordedEvidence {
    fn record(
        &self,
        evidence: RouteEffectEvidence<SessionRef, CodexGeneration>,
    ) -> DeliveryFuture<'_, ()> {
        Box::pin(async move {
            self.0.lock().await.push(evidence);
            Ok(())
        })
    }
}

pub(super) struct StaleRunningClaimRoute(pub(super) Arc<ProviderAcpDeliveryRoute>);

impl SessionDeliveryRoute for StaleRunningClaimRoute {
    fn reachability(&self) -> collaboration_protocol::SessionReachability {
        collaboration_protocol::SessionReachability::ProviderAcp
    }

    fn claim(
        &self,
        _: &SessionRef,
    ) -> DeliveryFuture<'_, collaboration_contracts::NativeRouteClaim> {
        Box::pin(async { Ok(collaboration_contracts::NativeRouteClaim::holds()) })
    }

    fn presence(&self, target: &SessionRef) -> DeliveryFuture<'_, RoutePresence> {
        self.0.presence(target)
    }

    fn deliver<'a>(
        &'a self,
        request: PreparedDeliveryRequest,
        evidence: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult> {
        self.0.deliver(request, evidence)
    }

    fn reconcile_attempt(
        &self,
        context: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        self.0.reconcile_attempt(context)
    }
}

pub(super) fn target() -> SessionRef {
    SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from("0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89".to_owned())
                .expect("service ID"),
            endpoint_id: EndpointId::try_from("cursor-local".to_owned()).expect("endpoint"),
        },
        session_id: SessionId::try_from("fixture-session".to_owned()).expect("session"),
    }
}

pub(super) fn provider_binding(target: &SessionRef) -> ProviderBindingIdentity {
    ProviderBindingIdentity {
        endpoint: target.endpoint.clone(),
        binding_id: ProviderBindingId::try_from("cursor-fixture-binding".to_owned())
            .expect("binding"),
        runtime: ProviderRuntimeIdentity {
            provider: ProviderKind::Cursor,
            runtime_name: NonEmptyText::try_from("cursor-fixture".to_owned()).expect("runtime"),
            runtime_version: None,
        },
        transport: ProviderTransport::StdioAcp,
        generation: CodexGeneration {
            service_epoch: UuidIdentity::try_from(
                "1ff962c5-7fa3-4c18-a5ca-1bbe8db09e80".to_owned(),
            )
            .expect("epoch"),
            generation: GenerationNumber::try_from(1).expect("generation"),
        },
        capabilities: ProviderCapabilities::try_from(vec![ProviderCapability {
            name: ProviderCapabilityName::Prompt,
            status: ProviderCapabilityStatus::Supported,
            evidence: ProviderCapabilityEvidence::Advertised,
        }])
        .expect("capabilities"),
    }
}

pub(super) fn available_directory(
    target: &SessionRef,
    binding: &ProviderBindingIdentity,
) -> EndpointDirectory {
    let directory = EndpointDirectory::new(target.endpoint.service_id.clone());
    directory
        .publish(EndpointDescription {
            endpoint: target.endpoint.clone(),
            label: NonEmptyText::try_from("Cursor".to_owned()).expect("label"),
            availability: EndpointAvailability::Available {
                observed_at: ObservationTimestamp::try_from("2026-09-24T12:00:00Z".to_owned())
                    .expect("time"),
            },
            channels: vec![ChannelDescription::ExternalProvider {
                transport: ProviderTransport::StdioAcp,
                binding_id: binding.binding_id.clone(),
                binding_generation: binding.generation.generation,
                runtime: binding.runtime.clone(),
                capabilities: binding.capabilities.clone(),
            }],
        })
        .expect("endpoint");
    directory
}

pub(super) fn cursor_prompt_fixture(
    event_socket: &Path,
    captured_prepared_prompt: &Path,
) -> ExternalProviderLaunch {
    let script = format!(
        r#"
import json,socket,sys
request=json.loads(sys.stdin.readline())
print(json.dumps({{'jsonrpc':'2.0','id':request['id'],'result':{{'protocolVersion':1,'agentCapabilities':{{}},'agentInfo':{{'name':'cursor-fixture','version':'1'}}}}}})); sys.stdout.flush()
request=json.loads(sys.stdin.readline())
assert request['method']=='session/new'
print(json.dumps({{'jsonrpc':'2.0','id':request['id'],'result':{{'sessionId':'fixture-session'}}}})); sys.stdout.flush()
request=json.loads(sys.stdin.readline())
assert request['method']=='session/prompt'
with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as event:
 event.connect({:?})
 event.sendall(b'first')
 event.recv(1)
print(json.dumps({{'jsonrpc':'2.0','id':request['id'],'result':{{'stopReason':'end_turn'}}}})); sys.stdout.flush()
request=json.loads(sys.stdin.readline())
assert request['method']=='session/prompt'
with open({:?},'w') as capture:
 json.dump(request,capture)
with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as event:
 event.connect({:?})
 event.sendall(b'second')
print(json.dumps({{'jsonrpc':'2.0','id':request['id'],'result':{{'stopReason':'end_turn'}}}})); sys.stdout.flush()
sys.stdin.read()
"#,
        event_socket.display().to_string(),
        captured_prepared_prompt.display().to_string(),
        event_socket.display().to_string()
    );
    ExternalProviderLaunch {
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        executable: PathBuf::from("/usr/bin/python3"),
        arguments: vec!["-c".to_owned(), script],
        environment: Vec::new(),
    }
}

pub(super) fn refusing_load_fixture(load_marker: &Path, error_code: i32) -> ExternalProviderLaunch {
    let script = format!(
        r#"
import json,sys
request=json.loads(sys.stdin.readline())
print(json.dumps({{'jsonrpc':'2.0','id':request['id'],'result':{{'protocolVersion':1,'agentCapabilities':{{'loadSession':True}},'agentInfo':{{'name':'load-fixture','version':'1'}}}}}})); sys.stdout.flush()
for line in sys.stdin:
 request=json.loads(line)
 assert request['method']=='session/load', request['method']
 assert request['params']['sessionId']=='fixture-session'
 with open({:?},'a') as marker: marker.write('session/load\n')
 print(json.dumps({{'jsonrpc':'2.0','id':request['id'],'error':{{'code':{error_code},'message':'provider fixture refusal','data':{{'privateText':'must not escape'}}}}}})); sys.stdout.flush()
"#,
        load_marker.display().to_string(),
        error_code = error_code,
    );
    ExternalProviderLaunch {
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        executable: PathBuf::from("/usr/bin/python3"),
        arguments: vec!["-c".to_owned(), script],
        environment: Vec::new(),
    }
}

pub(super) fn no_load_fixture() -> ExternalProviderLaunch {
    let script = r#"
import json,sys
initialize=json.loads(sys.stdin.readline())
print(json.dumps({'jsonrpc':'2.0','id':initialize['id'],'result':{'protocolVersion':1,'agentCapabilities':{},'agentInfo':{'name':'no-load-fixture','version':'1'}}})); sys.stdout.flush()
for line in sys.stdin:
 request=json.loads(line)
 assert request['method']=='session/new', request['method']
 print(json.dumps({'jsonrpc':'2.0','id':request['id'],'result':{'sessionId':'fixture-session'}})); sys.stdout.flush()
"#;
    ExternalProviderLaunch {
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        executable: PathBuf::from("/usr/bin/python3"),
        arguments: vec!["-c".to_owned(), script.to_owned()],
        environment: Vec::new(),
    }
}

pub(super) fn exited_provider_fixture(
    exit_marker: &Path,
    release_address: std::net::SocketAddr,
) -> ExternalProviderLaunch {
    let script = format!(
        r#"
import json,socket,sys
request=json.loads(sys.stdin.readline())
print(json.dumps({{'jsonrpc':'2.0','id':request['id'],'result':{{'protocolVersion':1,'agentCapabilities':{{'loadSession':True}},'agentInfo':{{'name':'exited-provider-fixture','version':'1'}}}}}})); sys.stdout.flush()
with socket.create_connection(({:?}, {}), timeout=2) as release:
 assert release.recv(1)==b'X', 'initialization release missing'
with open({:?},'w') as marker: marker.write('provider exited')
sys.exit(0)
"#,
        release_address.ip().to_string(),
        release_address.port(),
        exit_marker.display().to_string()
    );
    ExternalProviderLaunch {
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        executable: PathBuf::from("/usr/bin/python3"),
        arguments: vec!["-c".to_owned(), script],
        environment: Vec::new(),
    }
}

pub(super) fn request(target: SessionRef, text: &str) -> PreparedDeliveryRequest {
    let push_id =
        PushId::try_from(AttemptId::generate().as_str().to_owned()).expect("UUIDv7 push id");
    let correlation =
        DeliveryCorrelationId::try_from(push_id.as_str().to_owned()).expect("push id correlation");
    let line = MessageText::try_from(format!(
        "✉️ sender · \"{text}\" · router://{}/push/{}",
        String::from(target.endpoint.service_id.clone()),
        push_id.as_str()
    ))
    .expect("prepared push line");
    PreparedDeliveryRequest {
        payload: PreparedPush {
            push_id,
            line,
            load_policy: LoadPolicy::MayLoad,
        },
        target,
        mode: MessageDelivery::Auto,
        precondition: DeliveryPrecondition::Unpinned,
        correlation,
        attempt: AttemptId::generate(),
    }
}

pub(super) fn assert_uuid_v7_provider_reference(
    detail: &str,
    code_label: &str,
    provider_code: i32,
) {
    assert!(
        detail.contains(&format!("{code_label} {provider_code}; reference ")),
        "provider code/reference missing: {detail}"
    );
    let correlation_reference = detail
        .split_once("reference ")
        .map(|(_, reference)| reference.trim_end_matches(')'))
        .expect("provider correlation reference");
    let correlation_uuid =
        uuid::Uuid::parse_str(correlation_reference).expect("UUIDv7 correlation reference");
    assert_eq!(correlation_uuid.get_version_num(), 7, "{detail}");
}
