//! The Host's real router selects provider and peer clients through the collaboration API.
#[path = "../support/session_message_route_fixture.rs"]
mod route_fixture;
use codex_router_host::{
    CollaborationRuntime, CollaborationRuntimeInputs, ExternalProviderLaunchBinding,
    ExternalProviderStartup,
};
use collaboration_client::CollaborationClient;
use collaboration_protocol::{EndpointId, EndpointRef, SessionId, SessionRef};
use route_fixture::{
    create_provider_target, prompt_and_approve_from_peer_provider, provider_fixture,
    wait_for_completed_provider_prompts,
};
use serde_json::Value;
use std::os::unix::fs::PermissionsExt as _;

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
            board_session: None,
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
