//! A request the API sheds at its request limit was not run. Each family client reports it as
//! its own typed `overloaded` failure with "retry later", never as a protocol violation or an
//! uncertain outcome. The served API runs one request at a time on its socket; a held call
//! takes that one, so every other call is shed.

use collaboration_application_test_support::native_registry;
use collaboration_client::{
    BoardClientError, ClientError, CollaborationAccess, CollaborationClient,
    NativeTransportConnection, NativeTransportError, OperationEffect,
    operation_failure_from_client_error, resolve_public_native,
};
use collaboration_contracts::MachineIdentity;
use collaboration_mcp::request_slot_holder::RequestSlotHolder;
use collaboration_mcp::test_support::{ServedCollaborationApi, TestServeOptions};
use collaboration_protocol::{EndpointId, OperationFailureKind, OperationId};
use collaboration_service::ApplicationFixture;

use collaboration_service::ProjectAdmission;
use collaboration_service::ProjectAdmissionProps;
use message_board::{BoardFailureKind, BoardNextAction};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

const SERVICE_ID: &str = "00000000-0000-4000-8000-000000000001";

/// A served API that runs one request at a time, with that one held by the slot holder.
struct SaturatedApi {
    served: Arc<ServedCollaborationApi>,
    client: CollaborationClient,
    holder: RequestSlotHolder,
    held: tokio::task::JoinHandle<()>,
    directory: tempfile::TempDir,
}

type TestResult<TValue> = Result<TValue, Box<dyn std::error::Error>>;

impl SaturatedApi {
    async fn start() -> TestResult<Self> {
        let clock = Arc::new(router_clock::SystemRuntimeClock::new());

        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")?;
        // A local, unenrolled machine: the Fleet client has no session and no central.
        let fleet = collaboration_runtime::FleetClient::new(
            &message_board_storage::FleetStore::open(
                &directory
                    .path()
                    .join(message_board_storage::FleetStore::FILE_NAME),
            )
            .await?,
            Arc::new(native_registry::PendingRegistration),
            None,
        );
        let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
            clock: clock.clone(),
            registry: native_registry::new_registry_with_clock(directory.path(), clock.clone())
                .await
                .map_err(|error| std::io::Error::other(error.to_string()))?,
            fleet: Arc::new(fleet),
            identity: MachineIdentity::new(
                collaboration_protocol::UuidIdentity::try_from(SERVICE_ID.to_owned())?,
                Some("overload proof"),
            )?,
        })?);
        let identity = ApplicationFixture::new(SERVICE_ID, "00000000-0000-4000-8000-000000000002")
            .map_err(std::io::Error::other)?
            .with_project_admission(admission);
        let (identity, holder) =
            RequestSlotHolder::install(identity, SERVICE_ID, directory.path()).await?;
        let served = Arc::new(ServedCollaborationApi::start_with(
            directory.path(),
            (identity).build(),
            TestServeOptions {
                concurrent_requests: Some(1),
                ..TestServeOptions::default()
            },
        )?);
        let client = served.client("overload-proof").await?;
        // The holding call keeps the only request slot once a probe sent after it is shed. A
        // probe that wins the slot first sheds the holding call instead, which then ends, so
        // it is started again.
        let held = tokio::time::timeout(Duration::from_secs(10), async {
            let mut held = Self::hold(&served, &holder);
            loop {
                let probe = served.call("endpoints_list", json!({})).await?;
                if probe.pointer("/error/data/kind") == Some(&json!("overloaded")) {
                    break Ok::<_, std::io::Error>(held);
                }
                if held.is_finished() {
                    held = Self::hold(&served, &holder);
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| "the holding call never held the only request slot")??;
        Ok(Self {
            served,
            client,
            holder,
            held,
            directory,
        })
    }

    fn hold(
        served: &Arc<ServedCollaborationApi>,
        holder: &RequestSlotHolder,
    ) -> tokio::task::JoinHandle<()> {
        let served = Arc::clone(served);
        let arguments = holder.arguments();
        tokio::spawn(async move {
            let _answer = served.call(RequestSlotHolder::TOOL, arguments).await;
        })
    }

    async fn stop(self) -> TestResult<()> {
        self.holder.release().await?;
        let _ended = self.held.await;
        if let Ok(served) = Arc::try_unwrap(self.served) {
            served.stop().await?;
        }
        Ok(())
    }
}

fn from_json<TValue: serde::de::DeserializeOwned>(value: Value) -> serde_json::Result<TValue> {
    serde_json::from_value(value)
}

#[tokio::test]
async fn a_shed_board_call_is_a_typed_retryable_board_overload() {
    let api = SaturatedApi::start().await.expect("saturated API");
    let shed = api
        .client
        .project_list(
            from_json(json!({"boardId":message_board::BoardId::generate(), "page": {}}))
                .expect("request JSON"),
        )
        .await;
    let Err(BoardClientError::Rejected(failure)) = shed else {
        panic!("expected a typed board rejection, got {shed:?}");
    };
    assert_eq!(failure.kind, BoardFailureKind::Overloaded);
    assert_eq!(failure.next_action, BoardNextAction::RetryLater);
    api.stop().await.expect("API stops");
}

#[tokio::test]
async fn a_shed_board_write_is_not_an_unknown_outcome() {
    let api = SaturatedApi::start().await.expect("saturated API");
    let shed = api
        .client
        .project_create(
            from_json(json!({
                "projectId": OperationId::generate(),
                "boardId":message_board::BoardId::generate(), "title": "Shed",
                "requestId":message_board::RequestId::generate(), "protocolVersion":1,"actingFor":null,
                "actor": {"kind": "human", "humanId": "owner"}
            }))
            .expect("request JSON"),
        )
        .await;
    let Err(BoardClientError::Rejected(failure)) = shed else {
        panic!("a shed write must not read as an unknown outcome: {shed:?}");
    };
    assert_eq!(failure.kind, BoardFailureKind::Overloaded);
    api.stop().await.expect("API stops");
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
