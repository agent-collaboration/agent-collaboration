//! A request the API sheds at its request limit was not run. The client reports it as the
//! retryable overload, never as a protocol violation or an uncertain outcome. The served API
//! runs one request at a time on its socket; a caught-up journal read waiting for new records
//! takes that one, so every other call is shed.

use collaboration_client::{
    ClientError, CollaborationAccess, CollaborationClient, NativeTransportConnection,
    NativeTransportError, OperationEffect, operation_failure_from_client_error,
    resolve_public_native,
};
use collaboration_mcp::test_support::{ServedCollaborationApi, TestServeOptions};
use collaboration_protocol::{EndpointId, OperationFailureKind};
use collaboration_service::ApplicationFixture;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

const SERVICE_ID: &str = "00000000-0000-4000-8000-000000000001";
const SERVICE_EPOCH: &str = "00000000-0000-4000-8000-000000000002";

fn codex_endpoint() -> Value {
    json!({"serviceId": SERVICE_ID, "endpointId": "codex-local"})
}

/// A served API that runs one request at a time, with that one held by a journal read.
struct SaturatedApi {
    served: Arc<ServedCollaborationApi>,
    client: CollaborationClient,
    journal: Arc<lifecycle_observation::LifecycleStore>,
    held: tokio::task::JoinHandle<()>,
    directory: tempfile::TempDir,
}

type TestResult<TValue> = Result<TValue, Box<dyn std::error::Error>>;

impl SaturatedApi {
    async fn start() -> TestResult<Self> {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")?;
        let journal = lifecycle_observation::ObservationJournal::open(
            &directory.path().join("session-registry.sqlite"),
            "00000000-0000-4000-8000-000000000003"
                .to_owned()
                .try_into()?,
        )
        .await?;
        let journal = Arc::new(lifecycle_observation::LifecycleStore::new(journal));
        let description = serde_json::from_value(json!({
            "endpoint":codex_endpoint(),"label":"Codex fixture",
            "availability":{"state":"available","observedAt":"2026-10-08T00:00:00Z"},
            "channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"codex-native.sock",
                "schemaDigest":null,"generation":{"serviceEpoch":SERVICE_EPOCH,"generation":1}}]
        }))?;
        let identity = ApplicationFixture::new(SERVICE_ID, SERVICE_EPOCH)
            .map_err(std::io::Error::other)?
            .with_endpoints(vec![description])
            .map_err(std::io::Error::other)?
            .with_journal(Arc::clone(&journal));
        let served = Arc::new(ServedCollaborationApi::start_with(
            directory.path(),
            identity.build(),
            TestServeOptions {
                concurrent_requests: Some(1),
                ..TestServeOptions::default()
            },
        )?);
        let client = served.client("overload-proof").await?;
        let status = served.call("journal_status", json!({})).await?;
        let journal_id = status
            .pointer("/result/bounds/journalId")
            .filter(|journal_id| journal_id.is_string())
            .cloned()
            .ok_or_else(|| format!("journal status failed: {status}"))?;
        let last_sequence = status
            .pointer("/result/bounds/lastSequence")
            .cloned()
            .ok_or_else(|| format!("journal status has no last sequence: {status}"))?;
        // The caught-up read holds the only request slot once a probe sent after it is shed. A
        // probe that wins the slot first would shed the read instead, so the read is started again.
        let held = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let holder = Arc::clone(&served);
                let read = json!({
                    "endpoint": codex_endpoint(),
                    "after": {"journalId": journal_id, "sequence": last_sequence},
                    "pageSize": 1,
                    "waitMilliseconds": 30000
                });
                let held = tokio::spawn(async move {
                    let _answer = holder.call("journal_read", read).await;
                });
                tokio::time::sleep(Duration::from_millis(100)).await;
                if held.is_finished() {
                    continue;
                }
                let probe = served.call("journal_status", json!({})).await?;
                if probe.pointer("/error/data/kind") == Some(&json!("overloaded")) {
                    break Ok::<_, std::io::Error>(held);
                }
                held.abort();
            }
        })
        .await
        .map_err(|_| "the journal read never held the only request slot")??;
        Ok(Self {
            served,
            client,
            journal,
            held,
            directory,
        })
    }

    async fn stop(self) -> TestResult<()> {
        self.held.abort();
        let _ended = self.held.await;
        if let Ok(served) = Arc::try_unwrap(self.served) {
            served.stop().await?;
        }
        if let Ok(journal) = Arc::try_unwrap(self.journal) {
            journal.close().await;
        }
        Ok(())
    }
}

#[tokio::test]
async fn a_shed_untyped_call_reports_no_effect_and_retry() {
    let api = SaturatedApi::start().await.expect("saturated API");
    let shed = api
        .client
        .list_endpoints()
        .await
        .expect_err("the endpoint list is shed");
    assert!(matches!(shed, ClientError::Overloaded { .. }), "{shed:?}");
    let conversation = shed
        .conversation_failure()
        .expect("the conversation family's overload");
    assert_eq!(serde_json::json!(conversation.kind), "overloaded");
    let failure = operation_failure_from_client_error(shed, OperationEffect::Unknown);
    assert_eq!(failure.kind, OperationFailureKind::Unavailable);
    assert_eq!(failure.service_kind.as_deref(), Some("overloaded"));
    assert_eq!(failure.effect, OperationEffect::None);
    assert!(failure.message.contains("not run"), "{}", failure.message);
    api.stop().await.expect("API stops");
}

#[tokio::test]
async fn shed_native_discovery_keeps_the_overload_for_the_bridge_and_the_launcher() {
    // Arrange
    let api = SaturatedApi::start().await.expect("saturated API");
    let directory = api.directory.path().to_owned();
    let endpoint_id = EndpointId::try_from("codex-local".to_owned()).expect("endpoint id");

    // Act: the native bridge's carrier discovery, and the hosted TUI launch's.
    let bridged =
        NativeTransportConnection::connect(&CollaborationAccess::api(&directory), endpoint_id)
            .await;
    let launched = tokio::task::spawn_blocking(move || resolve_public_native(&directory))
        .await
        .expect("launch discovery joins");

    // Assert: both name the retryable overload, not an undifferentiated discovery failure.
    let Err(bridge_error) = bridged else {
        panic!("the bridge's discovery was not shed");
    };
    assert!(
        matches!(bridge_error, NativeTransportError::Overloaded { .. }),
        "{bridge_error:?}"
    );
    let launch_error = launched.expect_err("the launch discovery is shed");
    assert!(
        matches!(
            launch_error
                .get_ref()
                .and_then(|source| source.downcast_ref::<NativeTransportError>()),
            Some(NativeTransportError::Overloaded { .. })
        ),
        "{launch_error:?}"
    );
    api.stop().await.expect("API stops");
}
