//! A real ACP load failure keeps its redacted trace reference in the direct delivery result.

use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use collaboration_protocol::{
    EndpointId, EndpointRef, ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess,
    SessionId, SessionRef,
};
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderSessionRecord;
use serde_json::Value;
use std::{
    io::{self, Write},
    os::unix::fs::PermissionsExt as _,
    path::Path,
    sync::{Arc, Mutex},
};
use tracing_subscriber::{fmt::MakeWriter, util::SubscriberInitExt};

const PRIVATE_PROVIDER_MARKER: &str = "ACP_PROVIDER_PRIVATE_MARKER_8F39B1";
const SENDER_SESSION_ID: &str = "provider-error-e2e-sender";
const TARGET_SESSION_ID: &str = "provider-error-e2e-target";
const MESSAGE_BODY: &str = "public message sent through the real ACP route";
const PROVIDER_ERROR_CODE: i64 = -32600;

#[derive(Clone, Default)]
struct CapturedTrace(Arc<Mutex<Vec<u8>>>);

impl CapturedTrace {
    fn rendered(&self) -> String {
        String::from_utf8_lossy(
            &self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
        .into_owned()
    }
}

struct CapturedTraceWriter(Arc<Mutex<Vec<u8>>>);

impl Write for CapturedTraceWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'writer> MakeWriter<'writer> for CapturedTrace {
    type Writer = CapturedTraceWriter;

    fn make_writer(&'writer self) -> Self::Writer {
        CapturedTraceWriter(Arc::clone(&self.0))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acp_load_failure_correlation_matches_direct_route_result() {
    let captured_trace = CapturedTrace::default();
    tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .without_time()
        .with_writer(captured_trace.clone())
        .finish()
        .try_init()
        .expect("install isolated ACP trace capture");

    let root = tempfile::tempdir().expect("Host root");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private Host root");
    let runtime = Box::pin(CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            directory: root.path().to_owned(),
            codex_home: root.path().to_owned(),
            backend_socket: root.path().join("backend.sock"),
            mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
            native_schema: None,
            peer_registry_directory: None,
            remote_control_server_name: None,
            owner_human_id: None,
        },
        vec![ExternalProviderStartup::Launch(
            ExternalProviderLaunchBinding::cursor(
                node_test_fixture::node_executable().expect("Node"),
                vec![
                    node_test_fixture::fixture_program("provider-load-error-agent.ts")
                        .to_string_lossy()
                        .into_owned(),
                    PRIVATE_PROVIDER_MARKER.to_owned(),
                ],
            )
            .expect("Cursor ACP binding"),
        )],
    ))
    .await
    .expect("real Host startup");

    let caller = SessionRef {
        endpoint: EndpointRef {
            service_id: runtime.service_id().clone(),
            endpoint_id: EndpointId::try_from("codex-local".to_owned()).expect("caller endpoint"),
        },
        session_id: SessionId::try_from(SENDER_SESSION_ID.to_owned()).expect("caller session"),
    };
    let target = SessionRef {
        endpoint: EndpointRef {
            service_id: runtime.service_id().clone(),
            endpoint_id: EndpointId::try_from("cursor-local".to_owned()).expect("target endpoint"),
        },
        session_id: SessionId::try_from(TARGET_SESSION_ID.to_owned()).expect("target session"),
    };
    record_unloaded_provider_session(root.path(), &target, &caller).await;

    use collaboration_contracts::{
        DeliveryPrecondition, LoadPolicy, SessionMessageDelivery, UnstoredAttemptEvidenceSink,
        layer_zero::{DeliveryRequest, PreparedPush},
    };
    let id = uuid::Uuid::now_v7().to_string();
    let line = format!(
        "Fixture message · {MESSAGE_BODY} · router://{}/push/{id}",
        String::from(target.endpoint.service_id.clone())
    );
    let request = DeliveryRequest {
        payload: PreparedPush {
            push_id: id.clone().try_into().unwrap(),
            line: line.try_into().unwrap(),
            load_policy: LoadPolicy::MayLoad,
        },
        target,
        mode: collaboration_protocol::MessageDelivery::Auto,
        precondition: DeliveryPrecondition::Unpinned,
        correlation: id.try_into().unwrap(),
        attempt: agent_automation::AttemptId::generate(),
    };
    let receipt = runtime
        .session_delivery()
        .expect("composed Host routes")
        .deliver(request, &UnstoredAttemptEvidenceSink)
        .await
        .expect("delivery result")
        .into_receipt();
    let collaboration_protocol::DeliveryOutcome::Rejected(rejection) = receipt.outcome else {
        panic!("expected rejected load, got {:?}", receipt.outcome);
    };
    let detail = rejection
        .detail
        .expect("public detail for provider refusal");
    let log_event = captured_trace
        .rendered()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| {
            event
                .pointer("/fields/raw_provider_text")
                .and_then(Value::as_str)
                == Some(PRIVATE_PROVIDER_MARKER)
        })
        .expect("captured ACP error trace");
    assert_eq!(
        log_event
            .pointer("/fields/provider_code")
            .and_then(Value::as_i64),
        Some(PROVIDER_ERROR_CODE)
    );
    let correlation_id = log_event
        .pointer("/fields/correlation_id")
        .and_then(Value::as_str)
        .expect("trace correlation ID");
    assert_eq!(
        uuid::Uuid::parse_str(correlation_id)
            .expect("UUIDv7 trace correlation")
            .get_version_num(),
        7
    );
    assert!(detail.contains(&format!("provider code {PROVIDER_ERROR_CODE}")));
    assert!(detail.contains(correlation_id));
    assert!(!detail.contains(PRIVATE_PROVIDER_MARKER));

    runtime.shutdown().await.expect("Host shutdown");
}

#[expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
async fn record_unloaded_provider_session(
    directory: &Path,
    target: &SessionRef,
    caller: &SessionRef,
) {
    let mut store = ProviderOperationStore::open(&directory.join("provider-operations.sqlite"))
        .await
        .expect("provider store");
    store
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from(directory.display().to_string())
                .expect("working directory"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: caller.clone().into(),
            approver: caller.clone().into(),
            updated_at_ms: 1,
        })
        .await
        .expect("record provider session");
}
