#[path = "provider_fixture_launch.rs"]
mod provider_fixture_launch;
use agent_automation::RouteEffectEvidence;
use codex_router_host::{ExternalProviderLaunch, LiveSessionOwnership, LiveSessionOwnershipCheck};
use collaboration_protocol::{
    AttemptId, ChannelDescription, CodexGeneration, DeliveryCorrelationId, EndpointAvailability,
    EndpointDescription, EndpointId, EndpointRef, GenerationNumber, MessageDelivery, MessageText,
    NonEmptyText, ObservationTimestamp, ProviderBindingId, ProviderBindingIdentity,
    ProviderCapabilities, ProviderCapability, ProviderCapabilityEvidence, ProviderCapabilityName,
    ProviderCapabilityStatus, ProviderKind, ProviderRuntimeIdentity, ProviderTransport, PushId,
    SessionId, SessionRef, UuidIdentity,
};
use collaboration_service::{
    AttemptEvidenceSink, DeliveryFuture, DeliveryPrecondition, EndpointDirectory,
    layer_zero::{DeliveryRequest, PreparedPush},
};
use std::path::Path;

pub(super) struct NoLivePeer;

impl LiveSessionOwnershipCheck for NoLivePeer {
    fn check<'a>(&'a self, _target: &'a SessionRef) -> DeliveryFuture<'a, LiveSessionOwnership> {
        Box::pin(async { Ok(LiveSessionOwnership::NotLive) })
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

pub(super) fn cursor_prompt_fixture(event_socket: &Path) -> ExternalProviderLaunch {
    provider_fixture_launch::launch(
        "provider-message-fifo-agent.ts",
        vec![
            "cursor".to_owned(),
            event_socket.to_string_lossy().into_owned(),
        ],
    )
}

pub(super) fn multiblock_queue_fixture(event_socket: &Path) -> ExternalProviderLaunch {
    provider_fixture_launch::launch(
        "provider-message-fifo-agent.ts",
        vec![
            "blocks".to_owned(),
            event_socket.to_string_lossy().into_owned(),
        ],
    )
}

pub(super) fn refusing_load_fixture(load_marker: &Path) -> ExternalProviderLaunch {
    provider_fixture_launch::launch(
        "provider-message-fifo-agent.ts",
        vec![
            "load".to_owned(),
            load_marker.to_string_lossy().into_owned(),
        ],
    )
}

pub(super) fn first_prompt_refusal_fixture(event_socket: &Path) -> ExternalProviderLaunch {
    provider_fixture_launch::launch(
        "provider-message-fifo-agent.ts",
        vec![
            "refusal".to_owned(),
            event_socket.to_string_lossy().into_owned(),
        ],
    )
}

pub(super) fn request(target: SessionRef, text: &str) -> DeliveryRequest {
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
    DeliveryRequest {
        payload: PreparedPush {
            push_id,
            line,
            load_policy: collaboration_service::LoadPolicy::MayLoad,
        },
        target,
        mode: MessageDelivery::Auto,
        precondition: DeliveryPrecondition::Unpinned,
        correlation,
        attempt: AttemptId::generate(),
    }
}
