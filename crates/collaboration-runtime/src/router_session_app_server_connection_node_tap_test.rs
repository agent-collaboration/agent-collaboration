//! N1 node taps on the real connection loop: a Codex CLI reply to a server request travels to
//! the approvals broker (N7), and a client notification without an id is ignored. Each case
//! records the whole connection from its first frame.
use super::node_tap_tests::wire::CliConnectionRecorder;
use super::node_tap_tests::{commands_seen_at_n4, new_context, state_with_commands};
use super::test_support::ScriptedSessionBackend;
use super::*;
use crate::{NativeControlBackend, NativeGenerationGate, ServiceInteractionBroker};
use codex_router_test_support::node_taps::{
    CatalogRowExpectation, EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode, RecordIntent,
    RecordLayer, TapCase,
};
use session_event_model::{
    ApprovalChoice, ApprovalEffect, ApprovalRequest, ApprovalScope, OfferedOption, OfferedOptionId,
    OfferedOptions, OptionsOrigin, PendingInteraction,
};
use tokio_tungstenite::client_async;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn node_tap_n1_approval_reply_reaches_the_approvals_broker() {
    // Arrange: a connection with the broker installed and one pending approval.
    let directory = tempfile::tempdir().expect("directory");
    let backend = ScriptedSessionBackend::new().expect("backend");
    let actor = ScriptedSessionBackend::actor().expect("actor");
    let native_endpoint = serde_json::from_value(json!({
        "serviceId":backend.endpoint.service_id,
        "endpointId":"claude-local"
    }))
    .expect("endpoint");
    let interaction_store = Arc::new(
        interaction_storage::InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .expect("store"),
    );
    let broker = ServiceInteractionBroker::load(
        backend
            .endpoint
            .service_id
            .as_str()
            .to_owned()
            .try_into()
            .expect("service id"),
        NativeControlBackend {
            endpoint: native_endpoint,
            gate: NativeGenerationGate::default(),
            codex_home: directory.path().to_path_buf(),
        },
        interaction_store,
    )
    .await
    .expect("broker");
    let context = Arc::new(
        RouterSessionAppServerContext::new(
            backend.endpoint.clone(),
            actor.clone(),
            Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
            Arc::clone(&backend) as Arc<dyn SessionEventHub>,
            super::test_support::model_catalog(tokio::sync::watch::channel(Vec::new()).1),
        )
        .with_interaction_broker(Arc::clone(&broker)),
    );
    let (client, server) = UnixStream::pair().expect("pair");
    let task = tokio::spawn(serve_router_session_app_server_connection(server, context));
    let (mut client, _) = client_async("ws://localhost/rpc", client)
        .await
        .expect("client");
    // The case records the whole connection from its first frame: the fixture's thread start
    // too.
    let mut case = TapCase::new(
        ProtocolNode::N1CliFace,
        "approval_reply_to_n7",
        "N1 node: approval replies -> N7",
    );
    let mut connection = CliConnectionRecorder::default();
    connection
        .send(
            &mut case,
            &mut client,
            json!({"id":1,"method":"thread/start","params":{"cwd":"/tmp"}}),
        )
        .await;
    case.mark_last_setup();
    let started = connection
        .next_frame(&mut case, &mut client)
        .await
        .expect("thread/start answer");
    case.mark_last_setup();
    assert!(started["result"]["thread"].is_object(), "{started}");
    let request = ApprovalRequest {
        request_id: "approval-1".into(),
        title: "Run pwd".into(),
        description: None,
        subject: Some(session_event_model::ApprovalSubject::Command {
            command: "pwd".into(),
            cwd: "/tmp".into(),
        }),
        options_origin: OptionsOrigin::AgentOffered,
        options: OfferedOptions::new(vec![OfferedOption {
            option_id: OfferedOptionId::new("allow-once").expect("option id"),
            label: "Allow once".into(),
            choice: ApprovalChoice::new(ApprovalEffect::Allow, ApprovalScope::Once),
        }])
        .expect("options"),
    };
    let decision = broker
        .request_typed_approval(
            backend.session.clone(),
            actor.clone(),
            request.clone(),
            CancellationToken::new(),
            CancellationToken::new(),
            None,
        )
        .await
        .expect("register approval");
    backend
        .events
        .send(collaboration_contracts::HubEvent {
            sequence: 1,
            event: SessionEvent::InteractionRequested {
                interaction: PendingInteraction::Approval {
                    approver: actor,
                    request: Box::new(request),
                },
            },
        })
        .expect("publish");
    // The CLI receives the server request; every frame before it is recorded as setup.
    let server_request = loop {
        let frame = connection
            .next_frame(&mut case, &mut client)
            .await
            .expect("frame before the server request");
        if frame["method"] == "item/commandExecution/requestApproval" {
            break frame;
        }
        case.mark_last_setup();
    };
    case.state_before(NodeStateSnapshot::new().field(
        "pending_approvals",
        broker.list_typed_approvals(true).await.len(),
    ));
    // Act: the CLI answers "accept".
    connection
        .send(
            &mut case,
            &mut client,
            json!({"id":server_request["id"],"result":{"decision":"accept"}}),
        )
        .await;
    // The broker's own receiver is what N7 resolves; it settles only if N1 forwarded the reply.
    let resolution = decision.await.expect("broker resolution");
    // The whole resolution value as the broker delivered it (it has no serde).
    let kind = match &resolution {
        crate::interaction_broker::TypedApprovalResolution::Selected(selection) => {
            format!("SelectApproval({})", selection.option_id.as_str())
        }
        crate::interaction_broker::TypedApprovalResolution::Cancelled => "Cancel".to_owned(),
    };
    let decision = json!({"debug": format!("{resolution:#?}")});
    case.produced_with(
        RecordLayer::TypedOutcome,
        MessageFamily::ApprovalDecision,
        EdgeParty::Node(ProtocolNode::N7ApprovalsBroker),
        &kind,
        decision,
    );
    case.state_after(NodeStateSnapshot::new().field(
        "pending_approvals",
        broker.list_typed_approvals(true).await.len(),
    ));
    // Every frame N1 sends after the reply, until the connection ends, is recorded too; the row
    // declares none.
    client.close(None).await.expect("close");
    while connection
        .next_frame(&mut case, &mut client)
        .await
        .is_some()
    {}
    drop(client);
    let _ended = task.await;
    // Assert
    let thread_id = backend.session.session_id.as_str();
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::CodexMessageToCli,
            EdgeParty::CodexCli,
            "item/commandExecution/requestApproval",
            json!({
                "id":format!("router:approval:{thread_id}:approval-1"),
                "method":"item/commandExecution/requestApproval",
                "params":{
                    "threadId":thread_id,"turnId":"","itemId":"approval-1",
                    "startedAtMs":"<volatile:startedAtMs>","reason":"Run pwd",
                    "availableDecisions":["accept","cancel"],"command":"pwd","cwd":"/tmp"
                }
            }),
        )
        .produces_with(
            MessageFamily::ApprovalDecision,
            EdgeParty::Node(ProtocolNode::N7ApprovalsBroker),
            "SelectApproval(allow-once)",
            json!({"debug": format!(
                "{:#?}",
                crate::interaction_broker::TypedApprovalResolution::Selected(
                    crate::interaction_broker::TypedApprovalSelection {
                        option_id: OfferedOptionId::new("allow-once").expect("option id"),
                        note: None,
                    }
                )
            )}),
        )
        .state_after(NodeStateSnapshot::new().field("pending_approvals", 0));
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_n1_ignored_client_notification_without_id() {
    // Arrange: a live connection loop over an in-process socket pair.
    let backend = ScriptedSessionBackend::new().expect("backend");
    let (client, server) = UnixStream::pair().expect("pair");
    let task = tokio::spawn(serve_router_session_app_server_connection(
        server,
        new_context(&backend),
    ));
    let (mut client, _) = client_async("ws://localhost/rpc", client)
        .await
        .expect("client");
    let mut case = TapCase::new(
        ProtocolNode::N1CliFace,
        "ignored_client_notification_without_id",
        "Dispositions: Ignored ClientNotificationWithoutId",
    );
    case.state_before(state_with_commands(0));
    let mut connection = CliConnectionRecorder::default();
    // Act: a notification with no id, then an orderly close; every frame N1 sends is recorded.
    connection
        .send(&mut case, &mut client, json!({"method":"initialized"}))
        .await;
    client.close(None).await.expect("close");
    while connection
        .next_frame(&mut case, &mut client)
        .await
        .is_some()
    {}
    task.await.expect("join").expect("connection ended cleanly");
    case.state_after(state_with_commands(commands_seen_at_n4(&backend).len()));
    // Assert: nothing is answered and no command is issued.
    let row = CatalogRowExpectation::new().state_after(state_with_commands(0));
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_n1_connection_refused_config_write() {
    // Arrange: a live connection loop over an in-process socket pair.
    let backend = ScriptedSessionBackend::new().expect("backend");
    let (client, server) = UnixStream::pair().expect("pair");
    let task = tokio::spawn(serve_router_session_app_server_connection(
        server,
        new_context(&backend),
    ));
    let (mut client, _) = client_async("ws://localhost/rpc", client)
        .await
        .expect("client");
    let mut case = TapCase::new(
        ProtocolNode::N1CliFace,
        "connection_refused_config_write",
        "Dispositions: Refused ConfigWrite (config/write)",
    );
    case.state_before(state_with_commands(0));
    let mut connection = CliConnectionRecorder::default();
    // Act: the test, playing the CLI, sends `config/write` on the wire; every frame N1 sends is
    // recorded until the connection ends. The pinned 0.166.0 CLI never sends `config/write`, so the
    // test's own frame is apparatus: its method violation is not a Router defect. The Router defect
    // under it (N1's method table names `config/write` and misses `config/value/write`) is an
    // inventory row (router_session_app_server_methods.rs:87, D1).
    connection
        .send(
            &mut case,
            &mut client,
            json!({"id":5,"method":"config/write","params":{"keyPath":"model","value":"x","mergeStrategy":"replace"}}),
        )
        .await;
    case.mark_last(RecordIntent::SyntheticApparatus);
    let answer = connection
        .next_frame(&mut case, &mut client)
        .await
        .expect("config/write answer");
    client.close(None).await.expect("close");
    while connection
        .next_frame(&mut case, &mut client)
        .await
        .is_some()
    {}
    task.await.expect("join").expect("connection ended cleanly");
    case.state_after(state_with_commands(commands_seen_at_n4(&backend).len()));
    // Assert: a visible -32000 refusal, no session command.
    assert!(answer.get("result").is_none(), "{answer}");
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::CodexClientRequestFromCli,
            EdgeParty::CodexCli,
            "error:-32000:config/write",
            json!({"id":5,"error":{"code":-32000,"message":"Saving Codex configuration/defaults is not supported on this ACP endpoint"}}),
        )
        .state_after(state_with_commands(0));
    case.finish(&row).assert_settled();
}
