//! Host-composed delivery routes with direct preparation and in-memory evidence.
#[path = "../support/session_message_route_fixture.rs"]
pub(super) mod route_fixture;
use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use collaboration_client::CollaborationClient;
use collaboration_protocol::{
    DeliveryOutcome, DeliveryRejectionReason, EndpointId, EndpointRef, NativeSessionScope,
    NativeSessionSource, NativeSessionView, ProviderRequestedPolicy, ProviderSessionListParams,
    ProviderWorkingDirectory, RouterAccess, SessionId, SessionRef,
};
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderSessionRecord;
use route_fixture::{
    create_provider_target, deliver, load_provider_target, native_delivery_fixture,
    provider_fixture, publish_peer,
};
use serde_json::Value;
use std::os::unix::fs::PermissionsExt as _;
use tokio::io::{AsyncBufReadExt as _, BufReader};

#[tokio::test]
async fn host_router_selects_peer_and_provider_without_cross_loading() {
    let root = tempfile::tempdir().expect("Host root");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private Host root");
    let registry = root.path().join("peer-registry");
    std::fs::create_dir(&registry).expect("peer registry");
    let executable = root.path().join("provider.ts");
    provider_fixture(&executable);
    let claude_loads = root.path().join("claude-loads.log");
    let cursor_loads = root.path().join("cursor-loads.log");
    let claude_prompts = root.path().join("claude-prompts.log");
    let cursor_prompts = root.path().join("cursor-prompts.log");
    let (export, native) = native_delivery_fixture(root.path()).await;
    let mut runtime = CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            directory: root.path().to_owned(),
            codex_home: root.path().to_owned(),
            backend_socket: root.path().join("backend.sock"),
            mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
            native_schema: Some(export.clone()),
            peer_registry_directory: Some(registry.clone()),
            remote_control_server_name: None,
            owner_human_id: None,
        },
        vec![
            ExternalProviderStartup::Launch({
                let mut binding = ExternalProviderLaunchBinding::claude(
                    executable.clone(),
                    vec![
                        "claude-held".to_owned(),
                        claude_loads.display().to_string(),
                        claude_prompts.display().to_string(),
                    ],
                )
                .expect("Claude binding");
                codex_router_test_support::global_claude_fixture::declare_global_claude_fixture(
                    root.path(),
                    &mut binding.launch.environment,
                )
                .expect("declare test-owned global Claude executable");
                binding
            }),
            ExternalProviderStartup::Launch(
                ExternalProviderLaunchBinding::cursor(
                    executable,
                    vec![
                        "cursor-held".to_owned(),
                        cursor_loads.display().to_string(),
                        cursor_prompts.display().to_string(),
                        "cursor_login".to_owned(),
                    ],
                )
                .expect("Cursor binding"),
            ),
        ],
    )
    .await
    .expect("Host startup");
    runtime
        .backend_ready(
            collaboration_protocol::ObservationTimestamp::try_from(
                "2026-09-24T00:00:00Z".to_owned(),
            )
            .expect("fixture timestamp"),
            Some(codex_router_host::BackendSchemaEvidence {
                running_executable: export.executable(),
                export: &export,
            }),
        )
        .await
        .expect("native generation ready");
    let client = CollaborationClient::connect(root.path(), "route-composition", "1")
        .await
        .expect("collaboration API client");
    let claude_endpoint = EndpointRef {
        service_id: runtime.service_id().clone(),
        endpoint_id: EndpointId::try_from("claude-local".to_owned()).expect("Claude endpoint"),
    };
    let cursor_endpoint = EndpointRef {
        service_id: runtime.service_id().clone(),
        endpoint_id: EndpointId::try_from("cursor-local".to_owned()).expect("Cursor endpoint"),
    };
    let actor = SessionRef {
        endpoint: EndpointRef {
            service_id: runtime.service_id().clone(),
            endpoint_id: EndpointId::try_from("codex-local".to_owned()).expect("actor endpoint"),
        },
        session_id: SessionId::try_from("route-test-actor".to_owned()).expect("actor session"),
    };
    let codex_receipt = deliver(&runtime, actor.clone()).await;
    assert!(matches!(
        codex_receipt.outcome,
        DeliveryOutcome::Started | DeliveryOutcome::StartedOrSteered
    ));
    assert_eq!(
        codex_receipt.reachability,
        Some(collaboration_protocol::SessionReachability::CodexAppServer),
    );
    let not_loaded = deliver(
        &runtime,
        SessionRef {
            endpoint: actor.endpoint.clone(),
            session_id: SessionId::try_from("unloaded".to_owned()).expect("unloaded fixture"),
        },
    )
    .await;
    assert!(
        matches!(not_loaded.outcome, DeliveryOutcome::NotSubmitted { reason, retryable: false }
        if reason == collaboration_contracts::NOT_LOADED_REASON)
    );
    let unloaded = SessionRef {
        endpoint: claude_endpoint.clone(),
        session_id: SessionId::try_from("recorded-unloaded".to_owned()).expect("session"),
    };
    let unloaded_cursor = SessionRef {
        endpoint: cursor_endpoint.clone(),
        session_id: unloaded.session_id.clone(),
    };
    let mut store = ProviderOperationStore::open(&root.path().join("provider-operations.sqlite"))
        .await
        .expect("provider store");
    for target in [&unloaded, &unloaded_cursor] {
        store
            .record_session(&ProviderSessionRecord {
                target: target.clone(),
                working_directory: ProviderWorkingDirectory::try_from(
                    root.path().display().to_string(),
                )
                .expect("working directory"),
                requested_policy: ProviderRequestedPolicy {
                    access: RouterAccess::WriteRestricted,
                },
                created_by: actor.clone().into(),
                approver: actor.clone().into(),
                updated_at_ms: 1,
                resume: Default::default(),
            })
            .await
            .expect("record unloaded provider session");
    }
    drop(store);

    let peer_socket = registry.join("peer.sock");
    publish_peer(&registry, &unloaded.session_id, 99, &peer_socket);
    let unsupported = deliver(&runtime, unloaded.clone()).await;
    assert!(matches!(unsupported.outcome,
        DeliveryOutcome::Rejected(rejection) if rejection.reason == DeliveryRejectionReason::LiveElsewhere));
    assert!(
        !claude_loads.exists(),
        "unsupported live peer allowed provider load"
    );

    publish_peer(&registry, &unloaded.session_id, 1, &peer_socket);
    let listener = tokio::net::UnixListener::bind(&peer_socket).expect("peer socket");
    let receiver = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("peer accepted");
        let mut lines = BufReader::new(stream).lines();
        let _auth: Value =
            serde_json::from_str(&lines.next_line().await.expect("auth line").expect("auth"))
                .expect("auth JSON");
        let user: Value =
            serde_json::from_str(&lines.next_line().await.expect("user line").expect("user"))
                .expect("user JSON");
        user
    });
    let peer = deliver(&runtime, unloaded.clone()).await;
    assert_eq!(peer.outcome, DeliveryOutcome::PeerMessageWritten);
    assert_eq!(
        peer.reachability,
        Some(collaboration_protocol::SessionReachability::ClaudeCodePeer)
    );
    assert!(
        receiver.await.expect("peer receiver")["message"]["content"]
            .as_str()
            .expect("peer content")
            .contains("routed message")
    );
    assert!(
        !claude_loads.exists(),
        "writable peer allowed provider load"
    );

    std::fs::remove_file(registry.join(format!("{}.json", std::process::id())))
        .expect("remove peer registry record");
    // Loading stays an explicit conversation operation. Both records retain their identity.
    for (target, load_marker) in [
        (&unloaded, &claude_loads),
        (&unloaded_cursor, &cursor_loads),
    ] {
        load_provider_target(&client, target.clone(), actor.clone(), root.path()).await;
        let loaded = deliver(&runtime, target.clone()).await;
        assert_eq!(loaded.outcome, DeliveryOutcome::Started);
        assert_eq!(
            loaded.reachability,
            Some(collaboration_protocol::SessionReachability::ProviderAcp)
        );
        assert_eq!(
            std::fs::read_to_string(load_marker).expect("load marker"),
            "load\n"
        );
    }

    let held_claude =
        create_provider_target(&client, claude_endpoint, actor.clone(), root.path()).await;
    let held_cursor = create_provider_target(&client, cursor_endpoint, actor, root.path()).await;
    publish_peer(
        &registry,
        &held_claude.session_id,
        1,
        &registry.join("unused-peer.sock"),
    );
    let claude = deliver(&runtime, held_claude.clone()).await;
    assert_eq!(claude.outcome, DeliveryOutcome::Started);
    assert_eq!(
        claude.reachability,
        Some(collaboration_protocol::SessionReachability::ProviderAcp)
    );
    let cursor = deliver(&runtime, held_cursor.clone()).await;
    assert_eq!(cursor.outcome, DeliveryOutcome::Started);
    assert_eq!(
        cursor.reachability,
        Some(collaboration_protocol::SessionReachability::ProviderAcp)
    );
    assert_eq!(
        std::fs::read_to_string(&claude_loads).expect("load marker"),
        "load\n",
        "held Claude was loaded again"
    );
    assert_eq!(
        std::fs::read_to_string(&cursor_loads).expect("load marker"),
        "load\n",
        "held Cursor was loaded again"
    );
    for (reopened, created) in [(&unloaded, &held_claude), (&unloaded_cursor, &held_cursor)] {
        let list = client
            .list_provider_sessions(ProviderSessionListParams {
                endpoint: created.endpoint.clone(),
                view: NativeSessionView::Stored,
                scope: NativeSessionScope::Any,
                source: NativeSessionSource::All,
                query: None,
                page_size: 100,
                cursor: None,
            })
            .await
            .expect("provider sessions list");
        for target in [reopened, created] {
            assert!(
                list.sessions
                    .iter()
                    .any(|session| session.target() == target)
            );
        }
    }

    runtime.shutdown().await.expect("Host shutdown");
    assert!(
        !native.is_finished(),
        "native fixture saw an unexpected request"
    );
    native.abort();
    assert!(
        native
            .await
            .expect_err("terminating the native fixture")
            .is_cancelled()
    );
}
