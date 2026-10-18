use crate::api_test_harness::{ServedApi, api_config};
use crate::mcp_server::CollaborationMcpServer;
use collaboration_client::CollaborationClient;
use collaboration_protocol::UuidIdentity;
use collaboration_service::{
    BoardAvailability, MachineIdentity, ServiceIdentity, SessionDeliveryRouter,
    SessionMessageDelivery, SubscriptionDeliveryService, SubscriptionDeliveryServiceProps,
    SystemSubscriptionClock, TargetPresenceProbe,
};
use message_board::{BoardId, Identity, PostId, ProjectId, RequestId, WorkspaceId};
use rmcp::{ServerHandler as _, ServiceExt as _, handler::client::ClientHandler};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::Mutex;

const SERVICE_ID: &str = "00000000-0000-4000-8000-000000000011";
const SERVICE_EPOCH: &str = "00000000-0000-4000-8000-000000000012";

#[derive(Clone)]
struct TestMcpClient;

impl ClientHandler for TestMcpClient {}

async fn call_registered_tool(
    server: &CollaborationMcpServer,
    request_context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    name: &str,
    arguments: Value,
) -> rmcp::model::CallToolResult {
    let mut arguments = arguments
        .as_object()
        .expect("MCP tool arguments are an object")
        .clone();
    if matches!(
        name,
        "thread_join" | "thread_subscribe" | "thread_unsubscribe" | "thread_post"
    ) {
        arguments.insert("requestId".into(), json!(RequestId::generate()));
        arguments.insert("protocolVersion".into(), json!(1));
        arguments.insert("resubmits".into(), Value::Null);
        if name != "thread_join" {
            arguments.insert("actingFor".into(), Value::Null);
        }
        arguments.insert("observedEpoch".into(), json!(1));
    }
    if name == "thread_wait" {
        arguments.insert("protocolVersion".into(), json!(1));
        if name != "thread_join" {
            arguments.insert("actingFor".into(), Value::Null);
        }
        let thread = arguments["filter"]["threads"][0]["thread"].clone();
        arguments.insert(
            "renewals".into(),
            json!([{"requestId":RequestId::generate(),"resubmits":null,
            "observedEpoch":1,"scope":{"kind":"thread","thread":{"thread":thread}}}]),
        );
    }
    let response = server
        .call_tool(
            rmcp::model::CallToolRequestParams::new(name.to_owned()).with_arguments(arguments),
            request_context,
        )
        .await
        .expect("MCP tool call");
    let rmcp::model::CallToolResponse::Complete(result) = response else {
        panic!("board tool returns one complete result")
    };
    result
}

#[tokio::test]
async fn subscription_tools_roundtrip_through_mcp_api_native_projects_and_local_progress() {
    use std::os::unix::fs::PermissionsExt;

    let temporary = tempfile::tempdir_in("/tmp").expect("private service directory");
    std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private service permissions");

    let registry = Arc::new(collaboration_service::ProjectStoreRegistry::new(
        temporary.path(),
    ));
    let admission = Arc::new(
        collaboration_service::ProjectAdmission::new(
            collaboration_service::ProjectAdmissionProps {
                registry: registry.clone(),
                fleet: Arc::new(
                    collaboration_service::FleetClient::open_local_stand_in(temporary.path())
                        .await
                        .expect("fleet"),
                ),
                leases: Arc::new(collaboration_service::LocalLeaseAuthority::default()),
                identity: MachineIdentity::new(
                    UuidIdentity::try_from(SERVICE_ID.to_owned()).expect("service"),
                    Some("mcp-subscription-test"),
                )
                .expect("machine"),
            },
        )
        .expect("admission"),
    );
    let push_store = Arc::new(Mutex::new(
        automation_storage::AutomationStore::open(&temporary.path().join("automation.sqlite"))
            .await
            .expect("real push SQLite store"),
    ));
    let delivery_router = Arc::new(SessionDeliveryRouter::new(Vec::new()));
    let delivery: Arc<dyn SessionMessageDelivery> = delivery_router.clone();
    let presence: Arc<dyn TargetPresenceProbe> = delivery_router;
    let service_id = UuidIdentity::try_from(SERVICE_ID.to_owned()).expect("service id");
    let subscription_service = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
        board_availability: BoardAvailability::Available(Arc::new(Mutex::new(
            admission.subscription_window_store(push_store.clone()),
        ))),
        push_store: push_store.clone(),
        delivery,
        presence: Arc::clone(&presence),
        machine_identity: MachineIdentity::new(service_id, Some("mcp-subscription-test"))
            .expect("machine identity"),
        clock: Arc::new(SystemSubscriptionClock),
    });
    subscription_service
        .start()
        .await
        .expect("subscription service start");

    let service_identity = ServiceIdentity::new(SERVICE_ID, SERVICE_EPOCH)
        .expect("service identity")
        .with_project_admission(admission.clone())
        .with_automation_store(push_store.clone())
        .with_subscription_delivery_service(subscription_service.clone(), presence);
    let application =
        collaboration_service::CollaborationApplication::new(service_identity.clone());
    // Setup goes through the CLIs' client on the service socket, as an operator would.
    let setup_api =
        ServedApi::service_socket(&api_config(application.clone(), temporary.path())).await;

    let actor: Identity = serde_json::from_value(json!({
        "kind":"session",
        "session":{
            "endpoint":{"serviceId":SERVICE_ID,"endpointId":"codex-local"},
            "sessionId":"mcp-subscription-reader"
        }
    }))
    .expect("session actor");
    let setup_actor: Identity = serde_json::from_value(json!({
        "kind":"human",
        "humanId":"mcp-subscription-owner"
    }))
    .expect("human setup actor");
    let setup_client = CollaborationClient::connect(temporary.path(), "subscription-test", "1")
        .await
        .expect("setup collaboration client");
    let header = || json!({"requestId":RequestId::generate(),"protocolVersion":1,"resubmits":null,"actingFor":null,"actor":setup_actor});
    let workspace_id = WorkspaceId::generate();
    let board_id = BoardId::generate();
    let project_id = ProjectId::generate();
    let mut request = header();
    request["workspaceId"] = json!(workspace_id);
    request["name"] = json!("MCP workspace");
    request["description"] = json!("");
    accepted(
        serde_json::to_value(
            setup_client
                .workspace_create(serde_json::from_value(request).unwrap())
                .await
                .unwrap(),
        )
        .unwrap(),
    );
    let mut request = header();
    request["workspaceId"] = json!(workspace_id);
    request["boardId"] = json!(board_id);
    request["name"] = json!("MCP board");
    request["description"] = json!("");
    request["kind"] = json!({"kind":"area"});
    accepted(
        serde_json::to_value(
            setup_client
                .board_create(serde_json::from_value(request).unwrap())
                .await
                .unwrap(),
        )
        .unwrap(),
    );
    let mut request = header();
    request["boardId"] = json!(board_id);
    request["projectId"] = json!(project_id);
    request["title"] = json!("MCP subscriptions");
    accepted(
        serde_json::to_value(
            setup_client
                .project_create(serde_json::from_value(request).unwrap())
                .await
                .unwrap(),
        )
        .unwrap(),
    );
    let mut grant = header();
    grant["projectId"] = json!(project_id);
    grant["observedEpoch"] = json!(1);
    grant["operation"] = json!({"kind":"roleGrant","subject":{"kind":"project","subject":project_id},"role":"lead","holder":setup_actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null});
    accepted(
        serde_json::to_value(
            setup_client
                .role_grant(serde_json::from_value(grant).unwrap())
                .await
                .unwrap(),
        )
        .unwrap(),
    );
    let root_post = PostId::generate();
    let root_message_id = message_board::ThreadId::from(root_post.clone());
    let mut request = header();
    request["postId"] = json!(root_post);
    request["projectId"] = json!(project_id);
    request["observedEpoch"] = json!(1);
    request["about"] = json!({"kind":"project","subject":{"line":"MCP subscription entry"}});
    request["text"] = json!("MCP subscription entry path");
    request["references"] = json!([]);
    request["role"] = Value::Null;
    request["watch"] = json!(false);
    request["addressees"] = json!([]);
    accepted(
        serde_json::to_value(
            setup_client
                .thread_create(serde_json::from_value(request).unwrap())
                .await
                .unwrap(),
        )
        .unwrap(),
    );
    setup_api.stop().await;

    let mcp_server =
        CollaborationMcpServer::for_application(application, temporary.path().to_owned());
    let handler = mcp_server.clone();
    let (server_transport, client_transport) = tokio::io::duplex(4096);
    let (running_server, running_client) = tokio::join!(
        mcp_server.serve(server_transport),
        TestMcpClient.serve(client_transport),
    );
    let running_server = running_server.expect("MCP server transport");
    let running_client = running_client.expect("MCP test client transport");
    let tools = running_client
        .peer()
        .list_all_tools()
        .await
        .expect("MCP tools/list catalog");
    let tool_names = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    for removed_tool in [
        "board_thread_listen",
        "board_thread_listen_show",
        "board_thread_listen_cancel",
    ] {
        assert!(
            !tool_names.contains(&removed_tool),
            "legacy MCP listen tool {removed_tool} must be absent"
        );
    }
    for replacement_tool in [
        "thread_subscribe",
        "thread_unsubscribe",
        "thread_subscriptions",
        "thread_wait",
    ] {
        assert!(
            tool_names.contains(&replacement_tool),
            "replacement MCP subscription tool {replacement_tool} must be present"
        );
    }
    let context_for = |request_id: i64| {
        rmcp::service::RequestContext::new(
            rmcp::model::NumberOrString::Number(request_id),
            running_server.peer().clone(),
        )
    };

    let joined = call_registered_tool(
        &handler,
        context_for(1),
        "thread_join",
        json!({
            "thread":{"thread":root_message_id},
            "actor":actor,
            "role":"participant",
            "watch":true,
            "mode":"poll"
        }),
    )
    .await;
    assert_ne!(joined.is_error, Some(true));
    accepted(joined.structured_content.expect("join result"));
    let shown = call_registered_tool(
        &handler,
        context_for(13),
        "thread_show",
        json!({"thread":{"thread":root_message_id},"reader":{"kind":"identified","reader":actor}}),
    )
    .await;
    assert_eq!(
        shown.structured_content.unwrap()["watchStatus"]["watching"],
        true
    );

    let listed = call_registered_tool(
        &handler,
        context_for(2),
        "thread_subscriptions",
        json!({"actor":actor}),
    )
    .await;
    assert_ne!(listed.is_error, Some(true));
    let listed = listed
        .structured_content
        .expect("subscriptions structured result");
    assert_eq!(listed["subscriptions"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["subscriptions"][0]["policy"]["mode"], "poll");

    let updated = call_registered_tool(
        &handler,
        context_for(3),
        "thread_subscribe",
        json!({
            "actor":actor,
            "scope":{"kind":"thread","thread":{"thread":root_message_id}},
            "policy":{"mode":"poll","whenIdle":"hold","quietSeconds":0,"capSeconds":0,"lifetimeSeconds":600}
        }),
    )
    .await;
    assert_ne!(updated.is_error, Some(true));
    accepted(updated.structured_content.expect("subscribe result"));
    let policies = call_registered_tool(
        &handler,
        context_for(14),
        "thread_subscriptions",
        json!({"actor":actor}),
    )
    .await;
    assert_eq!(
        policies.structured_content.unwrap()["subscriptions"][0]["policy"]["mode"],
        "poll"
    );

    let empty_wait = call_registered_tool(
        &handler,
        context_for(4),
        "thread_wait",
        json!({
            "actor":actor,
            "filter":{"kind":"roots","threads":[{"thread":root_message_id}]},
            "maxWaitSeconds":0
        }),
    )
    .await;
    assert_ne!(empty_wait.is_error, Some(true));
    assert_eq!(
        empty_wait
            .structured_content
            .expect("empty wait structured result")["outcome"]["kind"],
        "timedOut"
    );

    let posted = call_registered_tool(
        &handler,
        context_for(5),
        "thread_post",
        json!({
            "postId":PostId::generate(),
            "placement":{"kind":"thread","thread":{"thread":root_message_id}},
            "actor":setup_actor,
            "actingFor":null,
            "text":"thread reply body must stay private",
            "references":[],"addressees":[]
        }),
    )
    .await;
    assert_ne!(posted.is_error, Some(true), "{posted:?}");
    accepted(posted.structured_content.expect("post result"));
    let posts=call_registered_tool(&handler,context_for(15),"post_list",json!({"scope":{"kind":"thread","thread":{"thread":root_message_id}},"selection":{"kind":"latest"},"page":{"limit":10,"cursor":null}})).await;
    assert_eq!(
        posts.structured_content.unwrap()["page"]["records"][0]["text"],
        "thread reply body must stay private"
    );

    let wait_arguments = || {
        json!({
            "actor":actor,
            "filter":{"kind":"roots","threads":[{"thread":root_message_id}]},
            "maxWaitSeconds":1
        })
    };
    let (first_wait, second_wait) = tokio::join!(
        call_registered_tool(&handler, context_for(6), "thread_wait", wait_arguments()),
        call_registered_tool(&handler, context_for(7), "thread_wait", wait_arguments()),
    );
    assert_ne!(first_wait.is_error, Some(true));
    assert_ne!(second_wait.is_error, Some(true));
    let first_batch = first_wait
        .structured_content
        .expect("first concurrent wait result")["outcome"]["batch"]
        .clone();
    let second_batch = second_wait
        .structured_content
        .expect("second concurrent wait result")["outcome"]["batch"]
        .clone();
    assert_ne!(
        first_batch.is_null(),
        second_batch.is_null(),
        "exactly one concurrent wait claims the due batch"
    );
    let notice = if first_batch.is_null() {
        second_batch
    } else {
        first_batch
    };
    assert_eq!(notice["kind"], "notice");
    assert_eq!(notice["roots"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        notice["roots"][0]["threadId"].as_str(),
        Some(root_message_id.as_str())
    );
    assert_eq!(notice["roots"][0]["postCount"], 1);
    assert!(notice.get("body").is_none());
    assert!(notice["roots"][0].get("body").is_none());
    assert!(
        !notice["line"]
            .as_str()
            .expect("notice line")
            .contains("thread reply body must stay private")
    );

    let updated = call_registered_tool(
        &handler,
        context_for(8),
        "thread_subscribe",
        json!({
            "actor":actor,
            "scope":{"kind":"thread","thread":{"thread":root_message_id}},
            "policy":{"mode":"off","quietSeconds":0,"capSeconds":60,"lifetimeSeconds":600}
        }),
    )
    .await;
    assert_ne!(updated.is_error, Some(true));
    accepted(updated.structured_content.expect("off result"));
    let policies = call_registered_tool(
        &handler,
        context_for(16),
        "thread_subscriptions",
        json!({"actor":actor}),
    )
    .await;
    assert_eq!(
        policies.structured_content.unwrap()["subscriptions"][0]["policy"]["mode"],
        "off"
    );

    let off_mode_wait = call_registered_tool(
        &handler,
        context_for(9),
        "thread_wait",
        json!({
            "actor":actor,
            "filter":{"kind":"roots","threads":[{"thread":root_message_id}]},
            "maxWaitSeconds":0
        }),
    )
    .await;
    assert_ne!(off_mode_wait.is_error, Some(true));
    let off_mode_refusal = off_mode_wait
        .structured_content
        .expect("typed off-mode wait refusal");
    assert_eq!(off_mode_refusal["outcome"]["kind"], "refused");
    let off_mode_refusal = &off_mode_refusal["outcome"]["reason"];
    assert_eq!(off_mode_refusal["kind"], "invalidField");
    assert_eq!(off_mode_refusal["stage"], "validation");
    assert_eq!(off_mode_refusal["details"]["kind"], "fieldConstraint");
    assert_eq!(off_mode_refusal["details"]["field"], "filter");
    assert_eq!(
        off_mode_refusal["details"]["requirement"],
        "wait requires a poll subscription; use thread subscribe --mode poll"
    );

    let unsubscribed = call_registered_tool(
        &handler,
        context_for(10),
        "thread_unsubscribe",
        json!({
            "actor":actor,
            "scope":{"kind":"thread","thread":{"thread":root_message_id}}
        }),
    )
    .await;
    assert_ne!(unsubscribed.is_error, Some(true));
    let unsubscribed = unsubscribed
        .structured_content
        .expect("unsubscribe structured result");
    accepted(unsubscribed);
    let record = admission
        .subscription_window_store(push_store.clone())
        .get_thread_subscription_record(
            &actor,
            &message_board::SubscriptionScope::thread(root_message_id.clone()),
            subscription_service.subscription_clock().now(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.state(),
        message_board::SubscriptionState::Ended {
            reason: message_board::EndReason::Cancelled
        }
    );

    let remaining = call_registered_tool(
        &handler,
        context_for(11),
        "thread_subscriptions",
        json!({"actor":actor}),
    )
    .await;
    assert_ne!(remaining.is_error, Some(true));
    assert_eq!(
        remaining
            .structured_content
            .expect("remaining subscriptions")["subscriptions"],
        json!([])
    );

    let thread = call_registered_tool(
        &handler,
        context_for(12),
        "thread_show",
        json!({"thread":{"thread":root_message_id},"reader":{"kind":"identified","reader":actor}}),
    )
    .await;
    assert_ne!(thread.is_error, Some(true));
    assert!(
        thread.structured_content.expect("thread show result")["watchStatus"]["watching"]
            .as_bool()
            .unwrap_or(false),
        "unsubscribe leaves the Watch active"
    );

    running_client.cancel().await.expect("stop MCP test client");
    running_server.cancel().await.expect("stop MCP test server");
    subscription_service.shutdown().await;
}

fn accepted(value: Value) {
    assert_eq!(value["result"]["decision"]["kind"], "accepted", "{value}");
}
