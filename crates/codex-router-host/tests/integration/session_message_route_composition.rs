//! The Host's real router selects provider and peer clients through the collaboration API.
#[path = "../support/session_message_route_fixture.rs"]
mod route_fixture;
use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use collaboration_client::CollaborationClient;
use collaboration_protocol::{
    DeliveryOutcome, DeliveryRejectionReason, EndpointId, EndpointRef, ProviderRequestedPolicy,
    ProviderWorkingDirectory, RouterAccess, SessionId, SessionRef,
};
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderSessionRecord;
use route_fixture::{
    create_provider_target, load_provider_target, message, prompt_and_approve_from_peer_provider,
    provider_fixture, publish_peer, wait_for_completed_provider_prompts,
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
    let runtime = CollaborationRuntime::start_with_external_providers(
        CollaborationRuntimeInputs {
            directory: root.path().to_owned(),
            codex_home: root.path().to_owned(),
            backend_socket: root.path().join("backend.sock"),
            mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
            native_schema: None,
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
    let unloaded = SessionRef {
        endpoint: claude_endpoint.clone(),
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
            created_by: (actor.clone()).into(),
            approver: (actor.clone()).into(),
            updated_at_ms: 1,
        })
        .await
        .expect("record unloaded provider session");
    drop(store);

    let peer_socket = registry.join("peer.sock");
    publish_peer(&registry, &unloaded.session_id, 99, &peer_socket);
    let unsupported = client
        .send_message(message(unloaded.clone()))
        .await
        .expect("unsupported peer receipt");
    let collaboration_protocol::MessageSendResult::Local(unsupported) = unsupported else {
        panic!("local route returned remote sender custody");
    };
    assert!(matches!(unsupported.receipt.outcome,
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
    let peer = client
        .send_message(message(unloaded))
        .await
        .expect("peer receipt");
    let collaboration_protocol::MessageSendResult::Local(peer) = peer else {
        panic!("local route returned remote sender custody");
    };
    assert_eq!(peer.receipt.outcome, DeliveryOutcome::PeerMessageWritten);
    assert_eq!(
        peer.receipt.reachability,
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
    // DM sends use LoadedOnly; loading is an explicit conversation operation.
    load_provider_target(
        &client,
        SessionRef {
            endpoint: claude_endpoint.clone(),
            session_id: SessionId::try_from("recorded-unloaded".to_owned()).expect("session"),
        },
        actor.clone(),
        root.path(),
    )
    .await;
    let loaded = client
        .send_message(message(SessionRef {
            endpoint: claude_endpoint.clone(),
            session_id: SessionId::try_from("recorded-unloaded".to_owned()).expect("session"),
        }))
        .await
        .expect("loaded provider delivery receipt");
    let collaboration_protocol::MessageSendResult::Local(loaded) = loaded else {
        panic!("local route returned remote sender custody");
    };
    assert_eq!(loaded.receipt.outcome, DeliveryOutcome::Started);
    assert_eq!(
        loaded.receipt.reachability,
        Some(collaboration_protocol::SessionReachability::ProviderAcp)
    );
    assert_eq!(
        std::fs::read_to_string(&claude_loads).expect("load marker"),
        "load\n"
    );

    let held_claude =
        create_provider_target(&client, claude_endpoint, actor.clone(), root.path()).await;
    let held_cursor = create_provider_target(&client, cursor_endpoint, actor, root.path()).await;
    publish_peer(
        &registry,
        &held_claude.session_id,
        1,
        &registry.join("unused-peer.sock"),
    );
    let claude = client
        .send_message(message(held_claude.clone()))
        .await
        .expect("held Claude receipt");
    let collaboration_protocol::MessageSendResult::Local(claude) = claude else {
        panic!("local route returned remote sender custody");
    };
    assert_eq!(claude.receipt.outcome, DeliveryOutcome::Started);
    assert_eq!(
        claude.receipt.reachability,
        Some(collaboration_protocol::SessionReachability::ProviderAcp)
    );
    let cursor = client
        .send_message(message(held_cursor.clone()))
        .await
        .expect("held Cursor receipt");
    let collaboration_protocol::MessageSendResult::Local(cursor) = cursor else {
        panic!("local route returned remote sender custody");
    };
    assert_eq!(cursor.receipt.outcome, DeliveryOutcome::Started);
    assert_eq!(
        cursor.receipt.reachability,
        Some(collaboration_protocol::SessionReachability::ProviderAcp)
    );
    assert_eq!(
        std::fs::read_to_string(&claude_loads).expect("load marker"),
        "load\n",
        "held Claude was loaded again"
    );
    assert!(!cursor_loads.exists(), "held Cursor was loaded again");

    prompt_and_approve_from_peer_provider(
        &client,
        held_claude.clone(),
        held_cursor.clone(),
        &cursor_prompts,
    )
    .await;
    prompt_and_approve_from_peer_provider(
        &client,
        held_cursor.clone(),
        held_claude.clone(),
        &claude_prompts,
    )
    .await;
    runtime.shutdown().await.expect("Host shutdown");
}

#[tokio::test]
async fn host_composed_permission_push_preserves_selected_option_through_provider_completion() {
    let root = tempfile::tempdir().expect("Host permission root");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private Host root");
    let executable = root.path().join("permission-provider.ts");
    provider_fixture(&executable);
    let requester_prompts = root.path().join("requester-prompts.log");
    let approver_prompts = root.path().join("approver-prompts.log");
    let runtime = CollaborationRuntime::start_with_external_providers(
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
        vec![
            ExternalProviderStartup::Launch({
                let mut binding = ExternalProviderLaunchBinding::claude(
                    executable.clone(),
                    vec![
                        "permission-requester".to_owned(),
                        root.path()
                            .join("requester-loads.log")
                            .display()
                            .to_string(),
                        requester_prompts.display().to_string(),
                    ],
                )
                .expect("requester binding");
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
                        "permission-approver".to_owned(),
                        root.path().join("approver-loads.log").display().to_string(),
                        approver_prompts.display().to_string(),
                        "cursor_login".to_owned(),
                    ],
                )
                .expect("approver binding"),
            ),
        ],
    )
    .await
    .expect("real Host startup");
    let client = CollaborationClient::connect(root.path(), "host-permission-proof", "1")
        .await
        .expect("collaboration API client");
    let actor = SessionRef {
        endpoint: EndpointRef {
            service_id: runtime.service_id().clone(),
            endpoint_id: EndpointId::try_from("codex-local".to_owned()).expect("actor endpoint"),
        },
        session_id: SessionId::try_from("permission-proof-actor".to_owned()).expect("actor"),
    };
    let requester = create_provider_target(
        &client,
        EndpointRef {
            service_id: runtime.service_id().clone(),
            endpoint_id: EndpointId::try_from("claude-local".to_owned())
                .expect("requester endpoint"),
        },
        actor.clone(),
        root.path(),
    )
    .await;
    let approver = create_provider_target(
        &client,
        EndpointRef {
            service_id: runtime.service_id().clone(),
            endpoint_id: EndpointId::try_from("cursor-local".to_owned())
                .expect("approver endpoint"),
        },
        actor,
        root.path(),
    )
    .await;

    let stored_notice = prompt_and_approve_from_peer_provider(
        &client,
        requester,
        approver.clone(),
        &approver_prompts,
    )
    .await;
    assert_eq!(stored_notice.record.target, approver);
    assert_eq!(
        stored_notice.record.kind,
        collaboration_protocol::PushKind::Approval
    );
    let observed_decision = std::fs::read_to_string(&requester_prompts)
        .expect("provider permission observations")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|entry| entry.get("fixturePermissionDecision").cloned())
        .expect("decision reached the requesting provider");
    assert_eq!(observed_decision["id"], 91);
    assert_eq!(
        observed_decision["result"]["outcome"],
        serde_json::json!({"outcome":"selected", "optionId":"allow-once"})
    );
    wait_for_completed_provider_prompts(&requester_prompts, 1).await;
    runtime.shutdown().await.expect("Host shutdown");
}

/// Local messages need only the interaction store: a Host whose automation store cannot open
/// still delivers a direct message to a running Claude Code peer.
#[tokio::test]
async fn host_delivers_a_local_message_while_automation_storage_is_unreadable() {
    // Arrange: an unreadable automation store and one running peer.
    let root = tempfile::tempdir().expect("Host root");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private Host root");
    std::fs::write(root.path().join("automation.sqlite"), b"not a database")
        .expect("unreadable automation store");
    let registry = root.path().join("peer-registry");
    std::fs::create_dir(&registry).expect("peer registry");
    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        directory: root.path().to_owned(),
        codex_home: root.path().to_owned(),
        backend_socket: root.path().join("backend.sock"),
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        native_schema: None,
        peer_registry_directory: Some(registry.clone()),
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await
    .expect("Host startup");
    let client = CollaborationClient::connect(root.path(), "local-message", "1")
        .await
        .expect("collaboration API client");
    let target = SessionRef {
        endpoint: EndpointRef {
            service_id: runtime.service_id().clone(),
            endpoint_id: EndpointId::try_from("claude-local".to_owned()).expect("peer endpoint"),
        },
        session_id: SessionId::try_from("running-peer".to_owned()).expect("peer session"),
    };
    let peer_socket = registry.join("peer.sock");
    publish_peer(&registry, &target.session_id, 1, &peer_socket);
    let listener = tokio::net::UnixListener::bind(&peer_socket).expect("peer socket");
    let receiver = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("peer accepted");
        let mut lines = BufReader::new(stream).lines();
        let _auth = lines.next_line().await.expect("auth line").expect("auth");
        let user: Value =
            serde_json::from_str(&lines.next_line().await.expect("user line").expect("user"))
                .expect("user JSON");
        user
    });

    // Act
    let sent = client
        .send_message(message(target))
        .await
        .expect("local message admitted");

    // Assert: the peer received the message the Router stored under the returned id.
    let collaboration_protocol::MessageSendResult::Local(sent) = sent else {
        panic!("local route returned remote sender custody");
    };
    assert_eq!(sent.receipt.outcome, DeliveryOutcome::PeerMessageWritten);
    let received = receiver.await.expect("peer receiver");
    let content = received["message"]["content"]
        .as_str()
        .expect("peer content");
    assert!(content.contains("routed message"), "{content}");
    assert!(content.contains(sent.push_id.as_str()), "{content}");
    std::fs::remove_file(registry.join(format!("{}.json", std::process::id())))
        .expect("remove peer registry record");
    runtime.shutdown().await.expect("Host shutdown");
}

#[tokio::test]
async fn host_delivers_and_reads_local_session_mail_without_board_storage() {
    use collaboration_client::PublicMessageContent;
    use collaboration_protocol::{MessageShowResult, PushRecordShowParams};
    use tracing::instrument::WithSubscriber as _;

    for has_old_board_files in [false, true] {
        let router_root = tempfile::tempdir_in("/tmp").expect("Router root");
        let root = router_root.path().join("agent-communication");
        std::fs::create_dir(&root).expect("collaboration directory");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .expect("private Host root");
        let retired_paths = [
            "fleet.sqlite",
            "fleet-cache.sqlite",
            "fleet-outbox.sqlite",
            "board-cursors.sqlite",
            "projects/legacy/e1.sqlite",
            "board-read-cache/legacy.sqlite",
            "machine-session/state.json",
        ];
        let old_bytes = b"old board data that cannot be opened as a store";
        if has_old_board_files {
            for path in retired_paths.map(|path| root.join(path)) {
                std::fs::create_dir_all(path.parent().expect("retired path parent"))
                    .expect("retired data directory");
                std::fs::write(path, old_bytes).expect("retired data");
            }
        }
        let enrollment_inputs = legacy_enrollment_inputs(router_root.path(), &root)
            .expect("legacy enrollment and reset inputs");
        if has_old_board_files {
            for (path, bytes) in &enrollment_inputs {
                std::fs::create_dir_all(path.parent().expect("fixture parent")).unwrap();
                std::fs::write(path, bytes).unwrap();
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
        }
        let registry = root.join("peer-registry");
        std::fs::create_dir(&registry).expect("peer registry");
        let trace = HostStartupOutput::default();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(trace.clone())
            .finish();
        let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
            directory: root.clone(),
            codex_home: root.clone(),
            backend_socket: root.join("backend.sock"),
            mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
            native_schema: None,
            peer_registry_directory: Some(registry.clone()),
            remote_control_server_name: None,
            owner_human_id: None,
        })
        .with_subscriber(subscriber)
        .await
        .expect("Host startup without board storage");
        let output = String::from_utf8_lossy(&trace.0.lock().unwrap()).to_lowercase();
        assert!(
            !output.contains("enrollment") && !output.contains("reset"),
            "{output}"
        );
        let client = CollaborationClient::connect(&root, "local-board-free-mail", "1")
            .await
            .expect("discover the Host and connect");
        let target = SessionRef {
            endpoint: EndpointRef {
                service_id: runtime.service_id().clone(),
                endpoint_id: EndpointId::try_from("claude-local".to_owned())
                    .expect("peer endpoint"),
            },
            session_id: SessionId::try_from("recipient".to_owned()).expect("recipient session"),
        };
        let mut sender = target.clone();
        sender.session_id = SessionId::try_from("sender".to_owned()).expect("sender session");
        let peer_socket = registry.join("peer.sock");
        let listener = tokio::net::UnixListener::bind(&peer_socket).expect("peer socket");
        publish_peer(&registry, &target.session_id, 1, &peer_socket);
        let receiver = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("peer accepted");
            let mut lines = BufReader::new(stream).lines();
            let _auth = lines.next_line().await.expect("auth line").expect("auth");
            serde_json::from_str::<Value>(
                &lines.next_line().await.expect("user line").expect("user"),
            )
            .expect("user JSON")
        });

        let body = "local mail with no board storage";
        let mut request = message(target.clone());
        request.message = PublicMessageContent::Agent {
            sender,
            text: collaboration_protocol::MessageText::try_from(body.to_owned())
                .expect("valid message"),
        };
        let sent = client
            .send_message(request)
            .await
            .expect("local message admitted");

        let collaboration_protocol::MessageSendResult::Local(sent) = sent else {
            panic!("local message returned remote custody");
        };
        assert_eq!(sent.receipt.outcome, DeliveryOutcome::PeerMessageWritten);
        let received = receiver.await.expect("peer receiver");
        let content = received["message"]["content"]
            .as_str()
            .expect("peer content");
        assert!(content.contains(body), "{content}");
        assert!(content.contains(sent.push_id.as_str()), "{content}");
        let shown = client
            .router_show(PushRecordShowParams {
                caller: target,
                reference: sent.push_id.as_str().to_owned(),
            })
            .await
            .expect("recipient can read the delivered message");
        let MessageShowResult::Local(shown) = shown else {
            panic!("local message returned remote custody");
        };
        assert_eq!(shown.record.push_id, sent.push_id);
        assert_eq!(shown.record.body.as_deref(), Some(body));
        runtime.shutdown().await.expect("Host shutdown");
        for (path, bytes) in enrollment_inputs {
            if has_old_board_files {
                assert_eq!(std::fs::read(path).unwrap(), bytes);
            } else {
                assert!(!path.exists(), "Host created {}", path.display());
            }
        }
        assert!(!root.join("enrollment-reset.lock").exists());
        for path in retired_paths.map(|path| root.join(path)) {
            if has_old_board_files {
                assert_eq!(
                    std::fs::read(path).expect("retired data survived"),
                    old_bytes
                );
            } else {
                assert!(!path.exists(), "Host created {}", path.display());
            }
        }
    }
}

#[derive(Clone, Default)]
struct HostStartupOutput(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for HostStartupOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("startup trace lock poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for HostStartupOutput {
    type Writer = Self;
    fn make_writer(&'writer self) -> Self {
        self.clone()
    }
}

type LegacyEnrollmentInputs = [(std::path::PathBuf, Vec<u8>); 2];

fn legacy_enrollment_inputs(
    router_root: &std::path::Path,
    collaboration_root: &std::path::Path,
) -> Result<LegacyEnrollmentInputs, Box<dyn std::error::Error>> {
    use codex_router_secret_store::fleet_secret_key::{FleetSecretKey, FleetSecretKind};

    let installation = uuid::Uuid::now_v7().to_string();
    let generation = uuid::Uuid::now_v7().to_string();
    let access = FleetSecretKey::new(
        FleetSecretKind::AccessClientSecret,
        &installation,
        &generation,
    )?
    .secret_key()?;
    let credential = FleetSecretKey::new(
        FleetSecretKind::MachineCredential,
        &installation,
        &generation,
    )?
    .secret_key()?;
    // These are synthetic references. No credentials are stored or resolved.
    let enrollment = serde_json::json!({
        "endpoint":"ws://127.0.0.1:1/session",
        "access":{"clientId":"fixture", "clientSecretKey":access.as_str()},
        "credentialKey":credential.as_str(),
        "peer":uuid::Uuid::now_v7(),
        "receiver":uuid::Uuid::now_v7(),
        "relay":uuid::Uuid::now_v7(),
    });
    let pending_reset = serde_json::json!({
        "generation":collaboration_protocol::OperationId::generate(),
        "phase":{"kind":"pending"},
    });
    Ok([
        (
            router_root.join("fleet/enrollment.json"),
            serde_json::to_vec(&enrollment)?,
        ),
        (
            collaboration_root.join("enrollment-reset.json"),
            serde_json::to_vec(&pending_reset)?,
        ),
    ])
}

#[tokio::test]
async fn reopened_store_with_held_board_notification_delivers_and_reads_its_direct_message() {
    use collaboration_protocol::{
        DeliveryReceipt, MessageDelivery, MessageShowResult, PushHeaderFacts, PushKind, PushOrigin,
        PushRecordDraft, PushRecordShowParams, RouterOriginRef,
    };

    let root = tempfile::tempdir_in("/tmp").expect("Host root");
    let directory = root.path();
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
        .expect("private Host root");
    let registry = directory.join("peer-registry");
    std::fs::create_dir(&registry).unwrap();
    let target = SessionRef {
        endpoint: EndpointRef {
            service_id: collaboration_runtime::load_service_identity(directory).unwrap(),
            endpoint_id: EndpointId::try_from("claude-local".to_owned()).unwrap(),
        },
        session_id: SessionId::try_from("stored-mail-recipient".to_owned()).unwrap(),
    };
    let mut sender = target.clone();
    sender.session_id = SessionId::try_from("stored-mail-sender".to_owned()).unwrap();

    let body = "local DM restored beside old board mail";
    let notification_id: collaboration_protocol::PushId =
        uuid::Uuid::now_v7().to_string().try_into().unwrap();
    let message_id: collaboration_protocol::PushId =
        uuid::Uuid::now_v7().to_string().try_into().unwrap();
    let project = message_board::ProjectId::generate();
    let intent = message_board::NotifyIntentId::generate();
    let origin = RouterOriginRef::BoardIntent {
        project: project.clone(),
        intent: intent.clone(),
    };
    let store = interaction_storage::InteractionStore::open(directory.join("interaction.sqlite"))
        .await
        .unwrap();
    store
        .insert_push_record(PushRecordDraft {
            push_id: notification_id.clone(),
            kind: PushKind::BoardNotification,
            origin: PushOrigin::Router(PushKind::BoardNotification),
            origin_router_ref: Some(origin.canonical_string().unwrap()),
            target: target.clone(),
            mode: None,
            guard: None,
            reply_to_push_id: None,
            header_facts: PushHeaderFacts::BoardNotification {
                project,
                intent,
                cause: message_board::RoleAssignmentId::generate().into(),
                summary: "Legacy role notification".to_owned().try_into().unwrap(),
            },
            body: None,
            activity: None,
            created_at: "2026-11-19T00:00:00Z".parse().unwrap(),
        })
        .await
        .unwrap();
    store
        .hold_unsent_direct_message(
            &notification_id,
            DeliveryReceipt {
                outcome: DeliveryOutcome::NotSubmitted {
                    retryable: true,
                    reason: "legacy reader waiting for presence".to_owned(),
                },
                reachability: None,
                client: None,
            },
        )
        .await
        .expect("legacy held notification");
    store
        .insert_push_record(PushRecordDraft {
            push_id: message_id.clone(),
            kind: PushKind::DirectMessage,
            origin: PushOrigin::Session(sender),
            origin_router_ref: None,
            target: target.clone(),
            mode: Some(MessageDelivery::Auto),
            guard: None,
            reply_to_push_id: None,
            header_facts: PushHeaderFacts::DirectMessage {
                sender_display_name: None,
            },
            body: Some(body.to_owned()),
            activity: None,
            created_at: "2026-11-19T00:00:01Z".parse().unwrap(),
        })
        .await
        .unwrap();
    store.close().await.unwrap();

    let peer_socket = registry.join("peer.sock");
    let listener = tokio::net::UnixListener::bind(&peer_socket).unwrap();
    publish_peer(&registry, &target.session_id, 1, &peer_socket);
    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        directory: directory.to_owned(),
        codex_home: directory.to_owned(),
        backend_socket: directory.join("backend.sock"),
        mcp_bind: ([127, 0, 0, 1], 0).into(),
        native_schema: None,
        peer_registry_directory: Some(registry),
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await
    .expect("Host reopens existing mail store");
    let client = CollaborationClient::connect(directory, "reopened-local-mail", "1")
        .await
        .unwrap();
    let received = router_clock::timeout(
        &router_clock::SystemRuntimeClock::new(),
        std::time::Duration::from_secs(5),
        async {
            let (stream, _) = listener.accept().await.unwrap();
            let mut lines = BufReader::new(stream).lines();
            let _auth = lines.next_line().await.unwrap().unwrap();
            serde_json::from_str::<Value>(&lines.next_line().await.unwrap().unwrap()).unwrap()
        },
    )
    .await
    .expect("stored direct message reaches its peer");
    let content = received["message"]["content"].as_str().unwrap();
    assert!(content.contains(body), "{content}");
    assert!(content.contains(message_id.as_str()), "{content}");
    let shown = client
        .router_show(PushRecordShowParams {
            caller: target,
            reference: message_id.as_str().to_owned(),
        })
        .await
        .unwrap();
    let MessageShowResult::Local(shown) = shown else {
        panic!("stored local message returned remote custody");
    };
    assert_eq!(shown.record.push_id, message_id);
    assert_eq!(shown.record.body.as_deref(), Some(body));
    runtime
        .shutdown()
        .await
        .expect("Host shuts down with a reopened mail store");
}
