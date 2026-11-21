use super::*;
use crate::ExternalProviderStartup;
use collaboration_contracts::ProviderConversationBackend as _;
use collaboration_protocol::{
    ConversationCreateRequest, ConversationOperationSettlement, ConversationOperationWaitOutput,
    ConversationOperationWaitRequest, EndpointRef, OperationId, PositiveSeconds,
    ProviderRequestedPolicy, ProviderRequestedSettings, ProviderWorkingDirectory, RouterAccess,
};
use session_event_model::{InputId, PromptContent, ProviderSubmission};
use std::{os::unix::fs::PermissionsExt as _, sync::Arc, time::Duration};
use tokio::sync::oneshot;
use tokio_util::task::TaskTracker;

#[tokio::test]
async fn shutdown_drains_the_published_prompt_task_before_closing_the_store() {
    let PausedQueueSettlement {
        runtime,
        tasks,
        release_task,
        _root,
        ..
    } = pause_retired_queue_settlement().await;

    let shutdown = runtime.shutdown();
    tokio::pin!(shutdown);
    tokio::select! {
        biased;
        stopped = &mut shutdown => panic!("Host stopped with the operation task held: {stopped:?}"),
        () = wait_for_drain(&tasks) => {}
    }
    release_task.send(()).expect("release the settlement task");
    shutdown
        .await
        .expect("Host closes its store after the task exits");
}

#[tokio::test]
async fn shutdown_bounds_the_published_prompt_task_drain_on_the_router_clock() {
    let PausedQueueSettlement {
        runtime,
        tasks,
        release_task,
        clock,
        _root,
    } = pause_retired_queue_settlement().await;

    let shutdown = runtime.shutdown();
    tokio::pin!(shutdown);
    tokio::select! {
        biased;
        stopped = &mut shutdown => panic!("Host stopped with the operation task held: {stopped:?}"),
        () = wait_for_drain(&tasks) => {}
    }
    clock
        .advance(Duration::from_secs(10))
        .expect("reach the operation drain deadline");
    let error = shutdown
        .await
        .expect_err("the settlement task is still held");
    release_task.send(()).expect("release the settlement task");
    assert_eq!(
        error.to_string(),
        "external provider operation drain timed out"
    );
}

async fn wait_for_drain(tasks: &TaskTracker) {
    while !tasks.is_closed() {
        tokio::task::yield_now().await;
    }
}

struct PausedQueueSettlement {
    runtime: CollaborationRuntime,
    tasks: TaskTracker,
    release_task: oneshot::Sender<()>,
    clock: Arc<router_clock::ManualClock>,
    _root: tempfile::TempDir,
}

async fn pause_retired_queue_settlement() -> PausedQueueSettlement {
    let root = tempfile::tempdir().expect("private runtime root");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private root");
    let receipt = root.path().join("wire.json");
    let binding = ExternalProviderLaunchBinding::cursor(
        node_test_fixture::node_executable().expect("Node"),
        vec![
            node_test_fixture::fixture_program("cursor-settings-agent.ts")
                .display()
                .to_string(),
            "reactive".to_owned(),
            receipt.display().to_string(),
            "hold-first-prompt-open".to_owned(),
            "exit-on-stop-file".to_owned(),
        ],
    )
    .expect("fixture binding");
    let clock = Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH));
    let runtime = Box::pin(
        CollaborationRuntime::start_with_optional_router_proxy_endpoint(
            CollaborationRuntimeInputs {
                directory: root.path().to_owned(),
                codex_home: root.path().to_owned(),
                backend_socket: root.path().join("backend.sock"),
                mcp_bind: "127.0.0.1:0".parse().expect("loopback"),
                native_schema: None,
                peer_registry_directory: None,
                remote_control_server_name: None,
                owner_human_id: Some(
                    message_board::HumanId::try_from("fixture-owner".to_owned()).expect("owner"),
                ),
            },
            vec![ExternalProviderStartup::Launch(binding)],
            Arc::new(collaboration_mcp::FixedRouterExecutableRelation(
                RouterExecutableRelation::Match,
            )),
            None,
            None,
            clock.clone(),
        ),
    )
    .await
    .expect("Host starts");
    let supervisor = runtime
        .external_provider_supervisor
        .as_deref()
        .expect("provider supervisor");
    let route = runtime
        .provider_delivery_route
        .as_deref()
        .expect("provider delivery route");
    let actor: collaboration_protocol::ProviderIdentity =
        serde_json::from_value(serde_json::json!({"humanId":"fixture-owner"}))
            .expect("provider actor");
    let endpoint = EndpointRef {
        service_id: runtime.service_id.clone(),
        endpoint_id: EndpointId::try_from("cursor-local".to_owned()).expect("provider endpoint"),
    };
    let create = OperationId::generate();
    supervisor
        .create(ConversationCreateRequest {
            operation_id: create.clone(),
            endpoint,
            generation: None,
            working_directory: ProviderWorkingDirectory::try_from(
                root.path().display().to_string(),
            )
            .expect("working directory"),
            created_by: actor.clone(),
            approver: actor.clone(),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            settings: Some(ProviderRequestedSettings {
                mode: Some("auto".to_owned()),
                ..Default::default()
            }),
            work_roots: Vec::new(),
        })
        .await
        .expect("create admitted");
    let created = supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id: create,
            timeout_seconds: PositiveSeconds::try_from(10).expect("wait budget"),
        })
        .await
        .expect("create settles");
    let ConversationOperationWaitOutput::Available {
        settlement: ConversationOperationSettlement::Created { target, .. },
    } = created.output
    else {
        panic!("provider session was not created: {created:?}");
    };
    let submission = |text: &str| {
        ProviderSubmission::plain(vec![
            PromptContent::text(text.to_owned()).expect("prompt text"),
        ])
    };
    let first = route
        .prompt_contents(
            target.clone(),
            actor.clone(),
            InputId::generate(),
            submission("first"),
        )
        .await
        .expect("first prompt admitted");
    let before_queue = supervisor.live_operation_ids();
    assert!(before_queue.contains(&first.operation.operation_id));
    route
        .queue_contents(target, actor, submission("second"))
        .await
        .expect("second prompt queued");
    let after_queue = supervisor.live_operation_ids();
    let mut added = after_queue.difference(&before_queue);
    let queued = added.next().expect("one queued operation").clone();
    assert!(added.next().is_none(), "only the second input was added");
    let (published, release_task) = supervisor.pause_after_result_publication(queued.clone());

    node_test_fixture::release_fixture(&receipt, ".stop")
        .expect("retire provider while the prompt waits");
    published
        .await
        .expect("queued prompt task published its terminal result");
    let unsent = supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id: queued,
            timeout_seconds: PositiveSeconds::try_from(10).expect("wait budget"),
        })
        .await
        .expect_err("queued prompt was not sent");
    assert_eq!(
        unsent.effect,
        collaboration_protocol::ProviderOperationEffect::None
    );
    let tasks = supervisor.tracked_operation_tasks_for_test();
    PausedQueueSettlement {
        runtime,
        tasks,
        release_task,
        clock,
        _root: root,
    }
}
