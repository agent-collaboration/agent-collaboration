//! Host shutdown ends a follow delivery stalled in an external provider's `session/load`.
use crate::registered_route_project::route_project_requests;
use crate::session_follow_fixture::post_thread_activity_for_sessions;
use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use collaboration_client::CollaborationClient;
use collaboration_protocol::{
    EndpointId, EndpointRef, ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess,
    SessionId, SessionRef,
};
use collaboration_runtime::{ProviderOperationStore, ProviderSessionRecord};
use message_board::WhenIdle;
use std::os::unix::fs::PermissionsExt as _;
use tokio::io::AsyncReadExt as _;

/// The provider receives `session/load` and never answers it. Shutdown must still end:
/// the in-flight follow delivery is cut off and settles outcome-unknown.
#[tokio::test]
async fn shutdown_completes_while_follow_delivery_waits_on_unanswered_provider_load() {
    // Arrange: an enrolled Host whose Cursor provider never answers `session/load`.
    let root = tempfile::tempdir().expect("Host root");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private Host root");
    let identity = collaboration_contracts::MachineIdentity::new(
        collaboration_runtime::load_service_identity(root.path()).expect("Host service identity"),
        None,
    )
    .expect("Host machine label");
    let central = board_central_test_support::TestCentral::start_with_peer(
        &root.path().join("test-central"),
        &message_board::ActorOrigin {
            installation: String::from(identity.service_id().clone())
                .try_into()
                .expect("origin service"),
            machine: identity
                .machine_label()
                .as_str()
                .to_owned()
                .try_into()
                .expect("origin label"),
        },
        collaboration_runtime::load_installation_peer(root.path()).expect("installation peer"),
    )
    .await
    .expect("real central enrollment");
    let load_notice_socket = root.path().join("load-notice.sock");
    let load_notices =
        tokio::net::UnixListener::bind(&load_notice_socket).expect("load notice socket");
    let provider = root.path().join("stalled-load-provider.ts");
    node_test_fixture::write_node_fixture(
        &provider,
        "provider-stalled-load-agent.ts",
        &serde_json::json!({ "loadNoticeSocket": load_notice_socket }),
    )
    .expect("stalled-load provider fixture");
    let runtime = CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            board_session: Some(central.session_inputs().expect("enrolled board session")),
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
            ExternalProviderLaunchBinding::cursor(provider, Vec::new()).expect("Cursor binding"),
        )],
    )
    .await
    .expect("Host startup");
    let actor = SessionRef {
        endpoint: EndpointRef {
            service_id: runtime.service_id().clone(),
            endpoint_id: EndpointId::try_from("codex-local".to_owned()).expect("actor endpoint"),
        },
        session_id: SessionId::try_from("shutdown-test-actor".to_owned()).expect("actor"),
    };
    // A recorded but unloaded session: a follow batch with Wake must load it first.
    let unloaded = SessionRef {
        endpoint: EndpointRef {
            service_id: runtime.service_id().clone(),
            endpoint_id: EndpointId::try_from("cursor-local".to_owned()).expect("Cursor endpoint"),
        },
        session_id: SessionId::try_from("recorded-unloaded".to_owned()).expect("session"),
    };
    let mut store = ProviderOperationStore::open(&root.path().join("provider-operations.sqlite"))
        .await
        .expect("provider store");
    store
        .record_session(&ProviderSessionRecord {
            target: unloaded.clone(),
            working_directory: ProviderWorkingDirectory::try_from(
                root.path().display().to_string(),
            )
            .expect("working directory"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: actor.clone().into(),
            approver: actor.into(),
            updated_at_ms: 1,
        })
        .await
        .expect("record unloaded provider session");
    drop(store);
    let client = CollaborationClient::connect(root.path(), "shutdown-stalled-load", "1")
        .await
        .expect("collaboration API client");
    post_thread_activity_for_sessions(
        &client,
        [unloaded],
        Some(WhenIdle::Wake),
        route_project_requests(),
    )
    .await;
    let (mut notice, _) = load_notices.accept().await.expect("provider load notice");
    let mut received = String::new();
    notice
        .read_to_string(&mut received)
        .await
        .expect("read provider load notice");
    assert_eq!(
        received, "load\n",
        "the follow delivery reached session/load"
    );
    drop(client);

    // Act: shut the Host down while that load is still unanswered.
    let shutdown = runtime.shutdown().await;

    // Assert: shutdown returned rather than waiting on the provider.
    shutdown.expect("Host shutdown");
    central.shutdown().await.expect("central shutdown");
}
