#![expect(
    clippy::expect_used,
    reason = "A malformed fixed fixture must fail its test."
)]
//! Provider route decisions before any client effect.

use super::provider_fixture_launch;
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
use std::{path::Path, sync::Arc};

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
    provider_fixture_launch::launch(
        "provider-delivery-route-agent.ts",
        vec![
            "cursor".to_owned(),
            event_socket.to_string_lossy().into_owned(),
            captured_prepared_prompt.to_string_lossy().into_owned(),
        ],
    )
}

pub(super) fn refusing_load_fixture(load_marker: &Path, error_code: i32) -> ExternalProviderLaunch {
    provider_fixture_launch::launch(
        "provider-loaded-route-agent.ts",
        vec![
            "refusal".to_owned(),
            load_marker.to_string_lossy().into_owned(),
            error_code.to_string(),
        ],
    )
}

pub(super) fn no_load_fixture() -> ExternalProviderLaunch {
    provider_fixture_launch::launch("provider-loaded-route-agent.ts", vec!["new".to_owned()])
}

pub(super) fn exited_provider_fixture(
    exit_marker: &Path,
    release_address: std::net::SocketAddr,
) -> ExternalProviderLaunch {
    provider_fixture_launch::launch(
        "provider-loaded-route-agent.ts",
        vec![
            "exit".to_owned(),
            release_address.ip().to_string(),
            release_address.port().to_string(),
            exit_marker.to_string_lossy().into_owned(),
        ],
    )
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
