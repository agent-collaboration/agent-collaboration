use super::*;

#[test]
fn router_push_tools_expose_reported_caller_and_reference_contracts() {
    let server = CollaborationMcpServer::catalog_only();
    let tools = server.resolved_tools();

    for (name, required_fields) in [
        ("router_show", &["caller", "reference"][..]),
        ("message_inbox", &["caller", "limit"][..]),
        ("message_history", &["caller", "with", "limit"][..]),
        ("message_reply", &["caller", "reference", "text"][..]),
    ] {
        let tool = tools
            .iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("missing {name} tool"));
        let input_schema = Value::Object((*tool.input_schema).clone());
        let required = input_schema["required"]
            .as_array()
            .unwrap_or_else(|| panic!("{name} required input fields"));
        for field in required_fields {
            assert!(
                required.contains(&serde_json::json!(field)),
                "{name} must require {field}"
            );
        }
        assert!(
            input_schema["properties"].get("expectSender").is_none(),
            "{name} must not retain expectSender"
        );
        let description = tool.description.as_deref().unwrap_or_default();
        assert!(description.contains("reported"), "{name}: {description}");
        assert!(
            description.contains("not authenticated"),
            "{name}: {description}"
        );
    }

    let message_send = tools
        .iter()
        .find(|tool| tool.name == "message_send")
        .expect("message_send tool");
    let description = message_send.description.as_deref().unwrap_or_default();
    assert!(description.contains("reported"), "{description}");
    assert!(description.contains("not authenticated"), "{description}");
    let output_schema = Value::Object(
        (**message_send
            .output_schema
            .as_ref()
            .expect("message_send output schema"))
        .clone(),
    );
    let success_definition = output_schema["anyOf"][0]["$ref"]
        .as_str()
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
        .expect("message_send success definition");
    let alternatives = output_schema["$defs"][success_definition]["oneOf"]
        .as_array()
        .expect("closed local-or-queued result");
    assert_eq!(alternatives.len(), 2);
    let local_definition = alternatives[0]["$ref"]
        .as_str()
        .unwrap()
        .strip_prefix("#/$defs/")
        .unwrap();
    let required = output_schema["$defs"][local_definition]["required"]
        .as_array()
        .expect("message_send success fields");
    for field in ["pushId", "link", "target", "deliveryState", "receipt"] {
        assert!(
            required.contains(&serde_json::json!(field)),
            "message_send result must require {field}"
        );
    }
}

/// Session delivery that writes every message to a Claude Code peer.
struct PeerWrittenDelivery;

impl collaboration_contracts::SessionMessageDelivery for PeerWrittenDelivery {
    fn deliver<'a>(
        &'a self,
        _request: collaboration_contracts::layer_zero::DeliveryRequest,
        _evidence: &'a dyn collaboration_contracts::AttemptEvidenceSink,
    ) -> collaboration_contracts::DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult>
    {
        Box::pin(async {
            Ok(collaboration_contracts::NativeDeliveryResult::observed(
                collaboration_contracts::NativeObservedOutcome::PeerMessageWritten,
                Some(collaboration_protocol::SessionReachability::ClaudeCodePeer),
                Some(collaboration_protocol::DeliveryClientReceipt::ClaudeCodePeer),
            ))
        })
    }

    fn reconcile_attempt(
        &self,
        _context: collaboration_contracts::AttemptReconciliationContext,
    ) -> collaboration_contracts::DeliveryFuture<'_, collaboration_contracts::AttemptReconciliation>
    {
        Box::pin(async { Ok(collaboration_contracts::AttemptReconciliation::StillUnknown) })
    }
}

impl collaboration_contracts::TargetPresenceProbe for PeerWrittenDelivery {
    fn presence(
        &self,
        _: &collaboration_protocol::SessionRef,
    ) -> collaboration_contracts::DeliveryFuture<'_, collaboration_contracts::TargetPresence> {
        Box::pin(async { Ok(collaboration_contracts::TargetPresence::Running) })
    }
}

/// A server over a Router with a real push store, its delivery owner and peer delivery.
async fn message_server(
    directory: &std::path::Path,
) -> (
    CollaborationMcpServer,
    collaboration_service::SubscriptionDeliveryService,
    crate::native_mail_owner::NativeMailOwner,
) {
    let delivery: std::sync::Arc<dyn collaboration_contracts::SessionMessageDelivery> =
        std::sync::Arc::new(PeerWrittenDelivery);
    let presence: std::sync::Arc<dyn collaboration_contracts::TargetPresenceProbe> =
        std::sync::Arc::new(PeerWrittenDelivery);
    let interactions = std::sync::Arc::new(
        interaction_storage::InteractionStore::open(directory.join("interaction.sqlite"))
            .await
            .expect("native carrier owner"),
    );
    let identity = crate::api_test_harness::test_identity();
    let clock = identity.runtime_clock();
    let owner = collaboration_service::SubscriptionDeliveryService::new(
        collaboration_service::SubscriptionDeliveryServiceProps {
            push_store: interactions.clone(),
            delivery: std::sync::Arc::clone(&delivery),
            presence: std::sync::Arc::clone(&presence),
            machine_identity: collaboration_contracts::MachineIdentity::new(
                collaboration_protocol::UuidIdentity::try_from(FIXTURE_SERVICE_ID.to_owned())
                    .expect("service id"),
                Some("mcp-message-test"),
            )
            .expect("machine identity"),
            clock: clock.clone(),
        },
    );
    owner.start().await.expect("delivery owner starts");
    let (identity, native) = crate::native_mail_owner::start(
        identity,
        fleet_sync_protocol::PeerId::new(uuid::Uuid::now_v7()).expect("fixture peer"),
        collaboration_service::MailPushDeliveryProps {
            interactions,
            actors: owner.clone(),
            presence,
            machine: collaboration_contracts::MachineIdentity::new(
                FIXTURE_SERVICE_ID.to_owned().try_into().expect("service"),
                Some("mcp-message-test"),
            )
            .expect("machine"),
            clock: clock.clone(),
        },
    )
    .expect("native mail owner");
    (
        CollaborationMcpServer::for_application((identity).build(), directory.to_owned()),
        owner,
        native,
    )
}

#[tokio::test]
async fn direct_message_tools_store_show_list_and_reply_through_the_application() {
    // Arrange
    let temporary = tempfile::tempdir().expect("temporary directory");
    let (server, owner, native) = message_server(temporary.path()).await;
    let target = fixture_session("claude-local", "target-session");
    let sender = fixture_session("codex-local", "reported-sender");
    let request: super::super::message_tools::UndecodedMessageSend =
        serde_json::from_value(serde_json::json!({
            "target":target,
            "message":{"kind":"agent","sender":sender,"text":"reporting identity"},
            "delivery":"auto",
            "generationGuard":null
        }))
        .expect("message_send input");

    // Act
    let sent = server.message_send(super::super::Parameters(request)).await;
    let sent = sent.structured_content.expect("push result");
    let push_id = sent["pushId"]
        .as_str()
        .unwrap_or_else(|| panic!("stored push id: {sent}"))
        .to_owned();
    let inbox = server
        .message_inbox(super::super::Parameters(
            serde_json::from_value(serde_json::json!({"caller":target,"limit":20}))
                .expect("message_inbox input"),
        ))
        .await;
    let history = server
        .message_history(super::super::Parameters(
            serde_json::from_value(serde_json::json!({"caller":target,"with":sender,"limit":20}))
                .expect("message_history input"),
        ))
        .await;
    let shown = server
        .router_show(super::super::Parameters(
            serde_json::from_value(serde_json::json!({"caller":target,"reference":push_id}))
                .expect("router_show input"),
        ))
        .await;
    let reply = server
        .message_reply(super::super::Parameters(
            serde_json::from_value(serde_json::json!({
                "caller":target,
                "reference":format!("router://{FIXTURE_SERVICE_ID}/push/{push_id}"),
                "text":"reply by id"
            }))
            .expect("message_reply input"),
        ))
        .await;

    // Assert
    let success_fields = sent
        .as_object()
        .expect("push result object")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        success_fields,
        BTreeSet::from([
            "deliveryState",
            "link",
            "pushId",
            "receipt",
            "target",
            "targetIdentity"
        ]),
        "a success keeps the published, untagged push result"
    );
    assert_eq!(
        sent["link"],
        format!("router://{FIXTURE_SERVICE_ID}/push/{push_id}")
    );
    assert_eq!(sent["target"], target);
    assert_eq!(sent["receipt"]["outcome"]["kind"], "peerMessageWritten");
    let inbox = inbox.structured_content.expect("inbox result");
    assert_eq!(inbox["records"][0]["pushId"], push_id.as_str());
    let history = history.structured_content.expect("history result");
    assert_eq!(history["records"][0]["pushId"], push_id.as_str());
    assert_ne!(shown.is_error, Some(true));
    let shown = shown.structured_content.expect("shown record");
    assert_eq!(shown["record"]["origin"]["session"], sender);
    assert!(
        shown["record"]["readAt"].is_string(),
        "a show by the target marks the direct message read"
    );
    assert_ne!(reply.is_error, Some(true));
    let reply = reply.structured_content.expect("reply result");
    assert_eq!(
        reply["target"], sender,
        "a reply goes to the referenced sender"
    );
    assert_eq!(reply["receipt"]["outcome"]["kind"], "peerMessageWritten");
    drop(server);
    owner.shutdown().await;
    drop(owner);
    native.close().await.expect("close native mail owner");
}

#[tokio::test]
async fn router_show_by_an_unrelated_session_is_a_typed_not_permitted_error() {
    // Arrange
    let temporary = tempfile::tempdir().expect("temporary directory");
    let (server, owner, native) = message_server(temporary.path()).await;
    let request: super::super::message_tools::UndecodedMessageSend =
        serde_json::from_value(serde_json::json!({
            "target":fixture_session("claude-local", "target-session"),
            "message":{"kind":"agent","sender":fixture_session("codex-local", "sender"),"text":"private"},
            "delivery":"auto",
            "generationGuard":null
        }))
        .expect("message_send input");
    let sent = server.message_send(super::super::Parameters(request)).await;
    let push_id = sent.structured_content.expect("push result")["pushId"].clone();
    let caller = fixture_session("cursor-local", "unrelated-session");

    // Act
    let result = server
        .router_show(super::super::Parameters(
            serde_json::from_value(serde_json::json!({"caller":caller,"reference":push_id}))
                .expect("router_show input"),
        ))
        .await;

    // Assert
    assert_eq!(result.is_error, Some(true));
    let structured = result.structured_content.expect("typed read error");
    assert_eq!(structured["serviceKind"], "notPermitted");
    assert_eq!(structured["stage"], "inspect");
    assert_eq!(structured["effect"], "none");
    drop(server);
    owner.shutdown().await;
    drop(owner);
    native.close().await.expect("close native mail owner");
}

#[tokio::test]
async fn native_sessions_list_routes_claude_to_provider_tool_without_changing_schema() {
    let server = CollaborationMcpServer::catalog_only();
    let request: collaboration_protocol::NativeSessionListParams = serde_json::from_value(
        serde_json::json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},
            "view":"active","scope":{"kind":"any"},"source":"interactive",
            "pageSize":10
        }),
    ).expect("Claude list request");
    let result = server
        .sessions_list(super::super::Parameters(request))
        .await;
    assert_eq!(result.is_error, Some(true));
    let content = serde_json::to_string(&result.structured_content).expect("error content");
    assert!(content.contains("provider_sessions_list"), "{content}");
}

#[test]
fn message_send_route_push_results_match_advertised_output_schema() {
    let server = CollaborationMcpServer::catalog_only();
    let schema = server
        .resolved_tools()
        .into_iter()
        .find(|tool| tool.name == "message_send")
        .and_then(|tool| tool.output_schema)
        .expect("advertised message_send output schema");
    let schema = Value::Object((*schema).clone());
    assert!(schema["anyOf"].is_array(), "tool output must use anyOf");
    assert_eq!(
        schema.pointer("/$defs/McpToolError/oneOf/0/properties/mcpResult/const"),
        Some(&serde_json::json!("error")),
        "error branch must have an exclusive discriminator"
    );
    let validator = jsonschema::validator_for(&schema).expect("message_send JSON Schema");
    let target = fixture_session("codex-local", "thread-a");
    let generation = serde_json::json!({
        "serviceEpoch":"00000000-0000-4000-8000-000000000002","generation":1
    });
    let native = |acceptance: Value| {
        serde_json::json!({
            "outcome":{"kind":"startedOrSteered"},
            "reachability":"codexAppServer",
            "client":{
                "kind":"codexAppServer",
                "target":target,
                "generation":generation,
                "inputKind":"agent",
                "representation":"declaredAgentText",
                "clientUserMessageId":"message-a",
                "resumeEffect":"notRequested",
                "acceptance":acceptance
            }
        })
    };
    let cases = [
        (
            "codex-start",
            native(serde_json::json!({
                "kind":"nativeInputAccepted","operation":"turnStart",
                "disposition":"startedOrSteered","turnId":"turn-a"
            })),
        ),
        (
            "codex-steer",
            native(serde_json::json!({
                "kind":"steerAccepted","turnId":"turn-a"
            })),
        ),
        (
            "claude-peer",
            serde_json::json!({
                "outcome":{"kind":"peerMessageWritten"},"reachability":"claudeCodePeer",
                "client":{"kind":"claudeCodePeer"}
            }),
        ),
        (
            "provider-acp",
            serde_json::json!({
                "outcome":{"kind":"started"},"reachability":"providerAcp",
                "client":{"kind":"providerAcp","operationId":OperationId::generate()}
            }),
        ),
    ];
    for (route, receipt) in cases {
        let value = serde_json::json!({
            "pushId":FIXTURE_PUSH_ID,
            "link":format!("router://{FIXTURE_SERVICE_ID}/push/{FIXTURE_PUSH_ID}"),
            "target":target,
            "targetIdentity":"✳️ codex-local/threa",
            "deliveryState":"delivered",
            "receipt":receipt
        });
        let push_result: collaboration_protocol::PushMessageSendResult =
            serde_json::from_value(value.clone())
                .unwrap_or_else(|error| panic!("{route} push result fixture: {error}"));
        let result = super::super::message_receipt_result(
            collaboration_protocol::MessageSendResult::Local(push_result),
        );
        assert_ne!(result.is_error, Some(true), "{route} tool result");
        let structured = result.structured_content.expect("structured receipt");
        assert_eq!(structured, value, "{route} success wire shape changed");
        validator.validate(&structured).unwrap_or_else(|error| {
            panic!("{route} structuredContent violates advertised schema: {error}; {structured}")
        });
    }
}

#[test]
fn message_send_held_delivery_is_structured_success() {
    let link = format!("router://{FIXTURE_SERVICE_ID}/push/{FIXTURE_PUSH_ID}");
    let push_result: collaboration_protocol::PushMessageSendResult =
        serde_json::from_value(serde_json::json!({
            "pushId":FIXTURE_PUSH_ID,
            "link":link,
            "target":fixture_session("codex-local", "thread-a"),
            "targetIdentity":"✳️ codex-local/thread-a",
            "deliveryState":"held",
            "receipt":{
                "outcome":{"kind":"notSubmitted","retryable":true,"reason":"target is not running"},
                "reachability":null,
                "client":null
            }
        }))
        .expect("typed held push result");

    let result = super::super::message_receipt_result(
        collaboration_protocol::MessageSendResult::Local(push_result),
    );

    assert_ne!(result.is_error, Some(true));
    let structured = result.structured_content.expect("structured held success");
    assert_eq!(structured["deliveryState"], "held");
    assert_eq!(structured["link"], link);
    assert_eq!(structured["receipt"]["outcome"]["kind"], "notSubmitted");
}

#[test]
fn queued_send_and_reply_are_custody_successes_matching_the_advertised_schemas() {
    let server = CollaborationMcpServer::catalog_only();
    let expected = serde_json::json!({"queued":{
        "item":FIXTURE_PUSH_ID,
        "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000099","endpointId":"codex-local"},"sessionId":"remote-recipient"},
        "targetIdentity":"remote recipient"
    }});
    let send =
        super::super::message_receipt_result(serde_json::from_value(expected.clone()).unwrap());
    let reply = super::super::message_reply_receipt_result(
        serde_json::from_value(expected.clone()).unwrap(),
    );
    for (name, result) in [("message_send", send), ("message_reply", reply)] {
        assert_ne!(result.is_error, Some(true));
        assert_eq!(result.structured_content.as_ref(), Some(&expected));
        let schema = server
            .resolved_tools()
            .into_iter()
            .find(|tool| tool.name == name)
            .unwrap()
            .output_schema
            .unwrap();
        let schema = Value::Object((*schema).clone());
        let validator = jsonschema::validator_for(&schema).unwrap();
        assert!(
            validator.is_valid(&expected),
            "{name} refused queued custody"
        );
        let mut invented_receipt = expected.clone();
        invented_receipt["queued"]["receipt"] = serde_json::json!({"outcome":{"kind":"queued"}});
        assert!(
            !validator.is_valid(&invented_receipt),
            "{name} accepted a fabricated native receipt"
        );
    }
}

#[test]
fn message_reply_is_by_reference_and_has_no_latest_sender_compatibility_fields() {
    let server = super::super::CollaborationMcpServer::catalog_only();
    let tool = server
        .resolved_tools()
        .into_iter()
        .find(|tool| tool.name == "message_reply")
        .expect("advertised message_reply tool");
    let input_schema = serde_json::Value::Object((*tool.input_schema).clone());
    let required = input_schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .expect("reply required fields");
    assert!(required.iter().any(|field| field == "caller"));
    assert!(required.iter().any(|field| field == "reference"));
    assert!(required.iter().any(|field| field == "text"));
    assert!(input_schema["properties"].get("expectSender").is_none());
    let stale_latest_sender_input =
        serde_json::from_value::<collaboration_client::MessageReplyRequest>(serde_json::json!({
            "caller":fixture_session("codex-local", "target-session"),
            "reference":FIXTURE_PUSH_ID,
            "text":"reply",
            "expectSender":fixture_session("claude-local", "sender-session")
        }));
    assert!(
        stale_latest_sender_input.is_err(),
        "message_reply must reject expectSender instead of retaining a compatibility path"
    );

    let output_schema = serde_json::Value::Object(
        (**tool.output_schema.as_ref().expect("reply output schema")).clone(),
    );
    let validator = jsonschema::validator_for(&output_schema).expect("reply output schema");
    let valid_reply_result = serde_json::json!({
        "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"sender-session"},
        "targetIdentity":"✳️ claude-local/sender-s",
        "pushId":FIXTURE_PUSH_ID,
        "link":format!("router://{FIXTURE_SERVICE_ID}/push/{FIXTURE_PUSH_ID}"),
        "deliveryState":"delivered",
        "receipt":{"outcome":{"kind":"peerMessageWritten"},
            "reachability":"claudeCodePeer",
            "client":{"kind":"claudeCodePeer"}}
    });
    assert!(validator.is_valid(&valid_reply_result));
}

#[test]
fn message_reply_held_delivery_is_structured_success() {
    let link = format!("router://{FIXTURE_SERVICE_ID}/push/{FIXTURE_PUSH_ID}");
    let reply_result: collaboration_protocol::SessionMessageReplyResult =
        serde_json::from_value(serde_json::json!({
            "target":fixture_session("claude-local", "sender-session"),
            "targetIdentity":"✳️ claude-local/sender-session",
            "pushId":FIXTURE_PUSH_ID,
            "link":link,
            "deliveryState":"held",
            "receipt":{
                "outcome":{"kind":"notSubmitted","retryable":true,"reason":"target is not running"},
                "reachability":null,
                "client":null
            }
        }))
        .expect("typed held reply result");

    let result = super::super::message_reply_receipt_result(
        collaboration_protocol::MessageSendResult::Local(reply_result),
    );

    assert_ne!(result.is_error, Some(true));
    let structured = result.structured_content.expect("structured held success");
    assert_eq!(structured["deliveryState"], "held");
    assert_eq!(structured["link"], link);
    assert_eq!(structured["receipt"]["outcome"]["kind"], "notSubmitted");
}

#[test]
fn message_send_post_submission_failure_matches_advertised_output_schema() {
    let server = CollaborationMcpServer::catalog_only();
    let schema = server
        .resolved_tools()
        .into_iter()
        .find(|tool| tool.name == "message_send")
        .and_then(|tool| tool.output_schema)
        .expect("advertised message_send output schema");
    let validator = jsonschema::validator_for(&Value::Object((*schema).clone()))
        .expect("message_send JSON Schema");
    let target = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
        "sessionId":"thread-a"
    }))
    .expect("target");
    let stored_but_unknown = collaboration_service::collaboration_application::MessageFailure {
        existing: None,
        kind: collaboration_service::collaboration_application::MessageFailureKind::OutcomeUnknown,
        stage: collaboration_service::collaboration_application::MessageFailureStage::Store,
        message: "The message was stored; whether it was delivered is unknown.".to_owned(),
    };
    let result = super::super::failure_naming(
        &super::super::rejection_failure(&stored_but_unknown, OperationEffect::Unknown),
        "target",
        Some(target),
    );
    assert_eq!(result.is_error, Some(true));
    let structured = result.structured_content.expect("structured error receipt");
    validator.validate(&structured).unwrap_or_else(|error| {
        panic!(
            "post-submission structuredContent violates advertised schema: {error}; {structured}"
        )
    });
}

#[test]
fn message_send_unknown_delivery_retains_outcome_in_typed_error() {
    let server = CollaborationMcpServer::catalog_only();
    let schema = server
        .resolved_tools()
        .into_iter()
        .find(|tool| tool.name == "message_send")
        .and_then(|tool| tool.output_schema)
        .expect("advertised message_send output schema");
    let schema = Value::Object((*schema).clone());
    let validator = jsonschema::validator_for(&schema).expect("message_send JSON Schema");
    let push_result: collaboration_protocol::PushMessageSendResult =
        serde_json::from_value(serde_json::json!({
            "pushId":FIXTURE_PUSH_ID,
            "link":format!("router://{FIXTURE_SERVICE_ID}/push/{FIXTURE_PUSH_ID}"),
            "target":fixture_session("codex-local", "thread-a"),
            "targetIdentity":"✳️ codex-local/thread-a",
            "deliveryState":"outcome-unknown",
            "receipt":{"outcome":{"kind":"unknown"},"reachability":null,"client":null}
        }))
        .expect("typed unknown push result");
    let result = super::super::message_receipt_result(
        collaboration_protocol::MessageSendResult::Local(push_result),
    );
    assert_eq!(result.is_error, Some(true));
    let structured = result.structured_content.expect("structured error receipt");
    assert_eq!(structured["mcpResult"], "error");
    assert_eq!(structured["kind"], "outcomeUnknown");
    assert_eq!(structured["effect"], "unknown");
    assert_eq!(structured["pushId"], FIXTURE_PUSH_ID);
    assert_eq!(structured["receipt"]["outcome"]["kind"], "unknown");
    validator.validate(&structured).expect("typed error schema");
}

#[test]
fn message_send_foreign_writer_rejection_has_typed_action_in_mcp_error() {
    let server = CollaborationMcpServer::catalog_only();
    let schema = server
        .resolved_tools()
        .into_iter()
        .find(|tool| tool.name == "message_send")
        .and_then(|tool| tool.output_schema)
        .expect("advertised message_send output schema");
    let validator = jsonschema::validator_for(&Value::Object((*schema).clone()))
        .expect("message_send JSON Schema");
    let push_result: collaboration_protocol::PushMessageSendResult =
        serde_json::from_value(serde_json::json!({
            "pushId":FIXTURE_PUSH_ID,
            "link":format!("router://{FIXTURE_SERVICE_ID}/push/{FIXTURE_PUSH_ID}"),
            "target":fixture_session("codex-local", "thread-a"),
            "targetIdentity":"✳️ codex-local/thread-a",
            "deliveryState":"rejected",
            "receipt":{
                "outcome":{
                    "kind":"rejected","reason":"heldByAnotherClient",
                    "nextAction":"messageFromHoldingCodexClient","clientCode":-32600,
                    "detail":"Message it from the Codex client that holds it."
                },
                "reachability":"codexAppServer","client":null
            }
        }))
        .expect("typed rejected push result");
    let result = super::super::message_receipt_result(
        collaboration_protocol::MessageSendResult::Local(push_result),
    );
    assert_eq!(result.is_error, Some(true));
    let structured = result.structured_content.expect("structured MCP error");
    assert_eq!(structured["mcpResult"], "error");
    assert_eq!(structured["pushId"], FIXTURE_PUSH_ID);
    assert_eq!(
        structured["receipt"]["outcome"]["reason"],
        "heldByAnotherClient"
    );
    assert_eq!(
        structured["receipt"]["outcome"]["nextAction"],
        "messageFromHoldingCodexClient"
    );
    assert_eq!(
        structured["message"],
        "Message it from the Codex client that holds it."
    );
    validator
        .validate(&structured)
        .expect("typed MCP output schema");
}

#[tokio::test]
async fn message_rejections_before_submission_have_no_effect_and_keep_service_detail() {
    // Arrange
    let temporary = tempfile::tempdir().expect("temporary directory");
    let (server, owner, native) = message_server(temporary.path()).await;
    let foreign_sender = serde_json::json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-0000000000ff","endpointId":"codex-local"},
        "sessionId":"elsewhere"
    });
    let send: super::super::message_tools::UndecodedMessageSend =
        serde_json::from_value(serde_json::json!({
            "target":fixture_session("claude-local", "target-session"),
            "message":{"kind":"agent","sender":foreign_sender,"text":"wrong Router"},
            "delivery":"auto",
            "generationGuard":null
        }))
        .expect("message_send input");
    let remote_item = collaboration_protocol::MailItemId::generate();
    let foreign_target = serde_json::json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-0000000000ff","endpointId":"codex-local"},
        "sessionId":"foreign-target"
    });
    let remote = serde_json::from_value(serde_json::json!({
        "itemId": remote_item,
        "target": foreign_target,
        "message":{"kind":"humanUser","text":"Cannot leave this Router"}
    }))
    .expect("non-local message input");
    let reply: collaboration_client::MessageReplyRequest =
        serde_json::from_value(serde_json::json!({
            "caller":fixture_session("codex-local", "target-session"),
            "reference":"018f1f12-3456-7abc-8def-0123456789ff",
            "text":"reply to nothing"
        }))
        .expect("message_reply input");

    // Act
    let sent = server.message_send(super::super::Parameters(send)).await;
    let remote = server.message_send(super::super::Parameters(remote)).await;
    let replied = server.message_reply(super::super::Parameters(reply)).await;

    // Assert
    let sent = sent.structured_content.expect("send rejection");
    assert_eq!(sent["kind"], "rejected");
    assert_eq!(sent["serviceKind"], "wrongService");
    assert_eq!(sent["effect"], "none");
    assert!(sent["target"].is_null());
    assert_eq!(sent["code"], -32602);
    assert_eq!(sent["data"]["kind"], "wrongService");
    let remote = remote.structured_content.expect("non-local send rejection");
    assert_eq!(remote["serviceKind"], "unavailable");
    assert_eq!(remote["effect"], "none");
    let remote_push = remote_item.as_str().to_owned().try_into().expect("push id");
    assert!(
        native
            .interactions
            .get_push_record(&remote_push)
            .await
            .unwrap()
            .is_none(),
        "refused non-local send acquired local custody"
    );
    let replied = replied.structured_content.expect("reply rejection");
    assert_eq!(replied["serviceKind"], "notFound");
    assert_eq!(replied["effect"], "none");
    assert!(replied["caller"].is_null());
    drop(server);
    owner.shutdown().await;
    drop(owner);
    native.close().await.expect("close native mail owner");
}

#[test]
fn a_message_stored_without_a_known_delivery_names_its_target_with_unknown_effect() {
    let target: collaboration_protocol::SessionRef =
        serde_json::from_value(fixture_session("codex-local", "message-thread")).expect("target");
    let stored_but_unknown = collaboration_service::collaboration_application::MessageFailure {
        existing: None,
        kind: collaboration_service::collaboration_application::MessageFailureKind::OutcomeUnknown,
        stage: collaboration_service::collaboration_application::MessageFailureStage::Store,
        message: "The message was stored; whether it was delivered is unknown.".to_owned(),
    };

    let result =
        super::super::message_failure_result(&stored_but_unknown, "target", target.clone());

    let structured = result.structured_content.expect("structured failure");
    assert_eq!(structured["effect"], "unknown");
    assert_eq!(structured["target"], serde_json::json!(target));
    assert_eq!(structured["serviceKind"], "outcomeUnknown");
}

#[test]
fn native_control_errors_keep_service_detail_and_only_uncertain_effects_are_unknown() {
    let unsupported = super::super::structured_result::<serde_json::Value>(
        Err(collaboration_client::ClientError::Rejected {
            code: -32050,
            data: Some(serde_json::json!({
                "kind":"unsupportedCapability",
                "stage":"rename",
                "message":"Codex app-server method `thread/name/set` is missing from its cached schema"
            })),
        }),
        collaboration_protocol::OperationEffect::Unknown,
    );
    let unsupported = unsupported.structured_content.expect("unsupported detail");
    assert_eq!(
        unsupported["message"],
        "Codex app-server method `thread/name/set` is missing from its cached schema"
    );
    assert_eq!(unsupported["effect"], "none");
    assert_eq!(unsupported["data"]["kind"], "unsupportedCapability");

    let lost_after_send = super::super::structured_result::<serde_json::Value>(
        Err(collaboration_client::ClientError::Protocol(
            "connection closed after dispatch",
        )),
        collaboration_protocol::OperationEffect::Unknown,
    );
    let lost_after_send = lost_after_send
        .structured_content
        .expect("transport detail");
    assert_eq!(lost_after_send["effect"], "unknown");
    assert_eq!(
        lost_after_send["message"],
        "collaboration API protocol violation: connection closed after dispatch"
    );
}

#[tokio::test]
async fn local_message_history_uses_creation_order_before_applying_the_limit() {
    use collaboration_protocol::{
        MessageDelivery, MessageHistoryEntry, MessageHistoryResult, PushHeaderFacts, PushKind,
        PushOrigin, PushRecordDraft,
    };

    let temporary = tempfile::tempdir().unwrap();
    let (server, owner, native) = message_server(temporary.path()).await;
    let caller: collaboration_protocol::SessionRef =
        serde_json::from_value(fixture_session("codex-local", "history-caller")).unwrap();
    let other: collaboration_protocol::SessionRef =
        serde_json::from_value(fixture_session("claude-local", "history-other")).unwrap();
    let older_id: collaboration_protocol::PushId = "019f0000-0000-7000-8000-000000000002"
        .to_owned()
        .try_into()
        .unwrap();
    let newer_id: collaboration_protocol::PushId = "019f0000-0000-7000-8000-000000000001"
        .to_owned()
        .try_into()
        .unwrap();
    for (id, sender, target, created_at) in [
        (
            older_id.clone(),
            caller.clone(),
            other.clone(),
            "2026-11-19T00:00:00Z",
        ),
        (
            newer_id.clone(),
            other.clone(),
            caller.clone(),
            "2026-11-19T00:00:01Z",
        ),
    ] {
        native
            .interactions
            .insert_push_record(PushRecordDraft {
                push_id: id,
                target,
                kind: PushKind::DirectMessage,
                origin: PushOrigin::Session(sender),
                origin_router_ref: None,
                mode: Some(MessageDelivery::Auto),
                guard: None,
                reply_to_push_id: None,
                header_facts: PushHeaderFacts::DirectMessage {
                    sender_display_name: None,
                },
                body: Some("stored local history fixture".to_owned()),
                activity: None,
                created_at: created_at.parse().unwrap(),
            })
            .await
            .unwrap();
    }

    for (limit, expected) in [(2, vec![newer_id.clone(), older_id]), (1, vec![newer_id])] {
        let result = server
            .message_history(super::super::Parameters(
                collaboration_protocol::PushRecordHistoryParams {
                    caller: caller.clone(),
                    with: other.clone(),
                    limit,
                },
            ))
            .await;
        let history: MessageHistoryResult =
            serde_json::from_value(result.structured_content.unwrap()).unwrap();
        let ids: Vec<_> = history
            .records
            .into_iter()
            .map(|entry| {
                let MessageHistoryEntry::Local(entry) = entry else {
                    panic!("expected a local history entry");
                };
                entry.push_id
            })
            .collect();
        assert_eq!(ids, expected);
    }
    drop(server);
    owner.shutdown().await;
    drop(owner);
    native.close().await.unwrap();
}
