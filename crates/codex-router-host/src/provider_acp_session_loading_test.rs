use super::*;
use crate::{ExternalProviderBinding, ExternalProviderLaunch, ExternalProviderRuntime};
use collaboration_protocol::{
    CodexGeneration, EndpointId, EndpointRef, GenerationNumber, ProviderBindingId,
    ProviderBindingIdentity, ProviderCapabilities, ProviderCapability, ProviderCapabilityEvidence,
    ProviderCapabilityName, ProviderCapabilityStatus, ProviderKind, ProviderRequestedPolicy,
    ProviderRuntimeIdentity, ProviderTransport, ProviderWorkingDirectory, RouterAccess, SessionId,
    UuidIdentity,
};
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderSessionRecord;
use std::sync::Arc;

struct FixtureOwnership(LiveSessionOwnership);

impl LiveSessionOwnershipCheck for FixtureOwnership {
    fn check<'a>(
        &'a self,
        _target: &'a SessionRef,
    ) -> collaboration_contracts::DeliveryFuture<'a, LiveSessionOwnership> {
        let result = self.0;
        Box::pin(async move { Ok(result) })
    }
}

fn fixture_launch(event_path: &std::path::Path, fail_load: bool) -> ExternalProviderLaunch {
    crate::external_provider_runtime::acp_scripted_fixture::launch_fixture_agent(
        "host-fifo-load-agent.ts",
        vec![
            "load".to_owned(),
            event_path.to_string_lossy().into_owned(),
            fail_load.to_string(),
        ],
    )
}

fn target() -> SessionRef {
    SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from("0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89".to_owned())
                .expect("service"),
            endpoint_id: EndpointId::try_from("claude-local".to_owned()).expect("endpoint"),
        },
        session_id: SessionId::try_from("fixture-session".to_owned()).expect("session"),
    }
}

fn binding(target: &SessionRef) -> ProviderBindingIdentity {
    ProviderBindingIdentity {
        endpoint: target.endpoint.clone(),
        binding_id: ProviderBindingId::try_from("fixture-binding".to_owned()).expect("binding"),
        runtime: ProviderRuntimeIdentity {
            provider: ProviderKind::ClaudeCode,
            runtime_name: "fixture-agent".to_owned().try_into().expect("runtime name"),
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
            name: ProviderCapabilityName::Load,
            status: ProviderCapabilityStatus::Supported,
            evidence: ProviderCapabilityEvidence::Advertised,
        }])
        .expect("capabilities"),
    }
}

#[tokio::test]
async fn recorded_session_loads_without_creating_a_replacement() {
    let root = tempfile::tempdir().expect("fixture root");
    let event_path = root.path().join("provider-method.txt");
    let target = target();
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from(
                root.path().display().to_string(),
            )
            .expect("cwd"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: (target.clone()).into(),
            approver: (target.clone()).into(),
            updated_at_ms: 1,
        })
        .await
        .expect("record");
    let runtime = ExternalProviderRuntime::initialize(
        fixture_launch(&event_path, false),
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("runtime");
    let supervisor = ExternalProviderSupervisor::new(
        vec![ExternalProviderBinding {
            identity: binding(&target),
            runtime,
        }],
        Arc::clone(&store),
    )
    .expect("supervisor");

    let result = ensure_provider_session_loaded(
        &supervisor,
        &store,
        &FixtureOwnership(LiveSessionOwnership::NotLive),
        &target,
        LoadPolicy::MayLoad,
    )
    .await;

    assert!(
        matches!(result, ProviderSessionLoadOutcome::Ready),
        "{result:?}"
    );
    assert_eq!(
        std::fs::read_to_string(event_path).expect("method"),
        "session/load"
    );
    supervisor.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn live_peer_prevents_provider_load() {
    let root = tempfile::tempdir().expect("fixture root");
    let event_path = root.path().join("provider-method.txt");
    let target = target();
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from(
                root.path().display().to_string(),
            )
            .expect("cwd"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: (target.clone()).into(),
            approver: (target.clone()).into(),
            updated_at_ms: 1,
        })
        .await
        .expect("record");
    let runtime = ExternalProviderRuntime::initialize(
        fixture_launch(&event_path, false),
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("runtime");
    let supervisor = ExternalProviderSupervisor::new(
        vec![ExternalProviderBinding {
            identity: binding(&target),
            runtime,
        }],
        Arc::clone(&store),
    )
    .expect("supervisor");

    let result = ensure_provider_session_loaded(
        &supervisor,
        &store,
        &FixtureOwnership(LiveSessionOwnership::LiveWritable),
        &target,
        LoadPolicy::MayLoad,
    )
    .await;

    assert!(matches!(result, ProviderSessionLoadOutcome::LiveElsewhere));
    assert!(!event_path.exists());
    supervisor.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn rejected_load_preserves_provider_code_without_creating_replacement() {
    let root = tempfile::tempdir().expect("fixture root");
    let event_path = root.path().join("provider-method.txt");
    let target = target();
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from(
                root.path().display().to_string(),
            )
            .expect("cwd"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: (target.clone()).into(),
            approver: (target.clone()).into(),
            updated_at_ms: 1,
        })
        .await
        .expect("record");
    let runtime = ExternalProviderRuntime::initialize(
        fixture_launch(&event_path, true),
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("runtime");
    let supervisor = ExternalProviderSupervisor::new(
        vec![ExternalProviderBinding {
            identity: binding(&target),
            runtime,
        }],
        Arc::clone(&store),
    )
    .expect("supervisor");

    let result = ensure_provider_session_loaded(
        &supervisor,
        &store,
        &FixtureOwnership(LiveSessionOwnership::NotLive),
        &target,
        LoadPolicy::MayLoad,
    )
    .await;

    assert!(
        matches!(&result, ProviderSessionLoadOutcome::Rejected { reason: ProviderSessionLoadRejection::ProviderRejected { code, .. } } if *code == -32001),
        "{result:?}"
    );
    assert_eq!(
        std::fs::read_to_string(event_path).expect("method"),
        "session/load"
    );
    supervisor.shutdown().await.expect("shutdown");
}
