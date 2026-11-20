use super::*;

#[test]
fn retired_cancel_tools_and_cli_commands_are_absent() {
    let server = CollaborationMcpServer::catalog_only();
    let tools = server.resolved_tools();
    for retired in ["message_cancel", "project_notification_cancel"] {
        assert!(
            tools.iter().all(|tool| tool.name != retired),
            "retired tool {retired} must not be advertised"
        );
    }
    assert_eq!(
        agent_collaboration::refuse_retired_command_group(
            &["message", "cancel", "--help"].map(Into::into)
        ),
        Some(2),
        "message cancel must be rejected by the argument parser"
    );
    assert_eq!(
        agent_collaboration::refuse_retired_command_group(
            &["project", "notification-cancel", "--help"].map(Into::into)
        ),
        Some(2),
        "project notification-cancel must be rejected by the argument parser"
    );
    assert!(tools.iter().any(|tool| tool.name == "installation_show"));
    assert!(tools.iter().any(|tool| tool.name == "conversation_cancel"));
}

const RETIRED_BOARD_TOOLS: [&str; 96] = [
    "request_effects",
    "workstream_create",
    "workstream_rename",
    "workstream_close",
    "task_create",
    "task_update",
    "task_move_workstream",
    "task_set_status",
    "task_link",
    "task_unlink",
    "task_duplicate",
    "task_replace",
    "subtask_add",
    "subtask_check",
    "subtask_uncheck",
    "subtask_move",
    "subtask_remove",
    "work_trail_add",
    "work_trail_redact",
    "role_grant",
    "role_release",
    "owner_grant_record",
    "role_replace_lead",
    "role_acknowledge_current",
    "project_publish_plan",
    "project_close",
    "project_describe",
    "fact_record",
    "thread_renew",
    "workstream_show",
    "workstream_list",
    "task_show",
    "task_list",
    "subtask_show",
    "subtask_list",
    "role_show",
    "role_list",
    "fact_show",
    "fact_list",
    "intent_show",
    "intent_list",
    "adr_create",
    "adr_transition",
    "adr_show",
    "adr_list",
    "feed_read",
    "view_work",
    "view_card",
    "view_session_context",
    "view_owner",
    "view_lead_load",
    "work_trail_show",
    "discovery_search",
    "post_search",
    "workspace_create",
    "workspace_archive",
    "workspace_update",
    "workspace_show",
    "workspace_list",
    "repository_attach",
    "repository_detach",
    "repository_list",
    "board_create",
    "board_update",
    "board_show",
    "board_list",
    "board_close",
    "board_archive",
    "project_claim",
    "project_create",
    "project_update",
    "project_list",
    "project_show",
    "thread_post",
    "post_show",
    "post_list",
    "thread_show",
    "thread_resolve",
    "thread_unresolve",
    "thread_watch",
    "thread_unwatch",
    "project_watch",
    "project_unwatch",
    "thread_list",
    "thread_create",
    "thread_refile",
    "thread_join",
    "thread_leave",
    "thread_participant_list",
    "thread_subscribe",
    "thread_unsubscribe",
    "thread_subscriptions",
    "inbox_fetch",
    "inbox_acknowledge",
    "inbox_workspaces",
    "thread_wait",
];

/// The local wake and schedule tools this build no longer carries.
const RETIRED_WAKE_AND_SCHEDULE_TOOLS: [&str; 36] = [
    "instruction_create",
    "instruction_update",
    "instruction_show",
    "delivery_show",
    "schedule_import",
    "schedule_export",
    "schedule_create",
    "schedule_update",
    "schedule_show",
    "schedule_enable",
    "schedule_disable",
    "schedule_prepare",
    "run_show",
    "run_summary_retry",
    "run_summary_skip",
    "automation_configure",
    "run_reconcile",
    "delivery_reconcile",
    "operation_show",
    "operation_reconcile",
    "automation_events",
    "delivery_attempts",
    "run_summaries",
    "instruction_list",
    "schedule_list",
    "run_list",
    "delivery_list",
    "revision_list",
    "automation_status",
    "wake_send",
    "wake_show",
    "wake_pause",
    "wake_resume",
    "wake_cancel",
    "wake_list",
    "wake_wait_until_first_fire",
];

const RETIRED_MAIL_AND_INTERACTION_TOOLS: [&str; 9] = [
    "message_send",
    "message_reply",
    "message_inbox",
    "message_history",
    "router_show",
    "approval_list",
    "approval_decide",
    "question_list",
    "question_answer",
];

#[test]
fn retired_tools_are_absent_from_the_catalog() {
    let tools = CollaborationMcpServer::catalog_only().resolved_tools();
    for retired in RETIRED_WAKE_AND_SCHEDULE_TOOLS
        .into_iter()
        .chain(RETIRED_BOARD_TOOLS)
        .chain(RETIRED_MAIL_AND_INTERACTION_TOOLS)
    {
        assert!(
            tools.iter().all(|tool| tool.name != retired),
            "retired tool {retired} must not be advertised"
        );
    }
    assert!(
        tools
            .iter()
            .any(|tool| tool.name == "conversation_operation_show"),
        "the provider conversation operation tools stay"
    );
}

#[tokio::test]
async fn a_retired_tool_gets_the_ordinary_unknown_tool_answer() {
    // Arrange
    let directory = tempfile::tempdir().expect("service directory");
    let api = crate::api_test_harness::ServedApi::tcp(&crate::api_test_harness::api_config(
        crate::api_test_harness::test_identity().build(),
        directory.path(),
    ))
    .await;

    for (index, retired) in RETIRED_WAKE_AND_SCHEDULE_TOOLS
        .into_iter()
        .chain(RETIRED_BOARD_TOOLS)
        .chain(RETIRED_MAIL_AND_INTERACTION_TOOLS)
        .enumerate()
    {
        // Act
        let (_, answer) = api
            .post(&serde_json::json!({
                "jsonrpc":"2.0","id":index,"method":"tools/call",
                "params":{"name":retired,"arguments":{}}
            }))
            .await;

        // Assert: the same answer as any name this Router never had.
        assert_eq!(answer["error"]["code"], -32602, "{retired}: {answer}");
        assert_eq!(
            answer["error"]["message"], "tool not found",
            "{retired}: {answer}"
        );
        assert!(answer["error"].get("data").is_none(), "{retired}: {answer}");
    }
    let (_, never_known) = api
        .post(&serde_json::json!({
            "jsonrpc":"2.0","id":99,"method":"tools/call",
            "params":{"name":"no_such_tool","arguments":{}}
        }))
        .await;
    assert_eq!(never_known["error"]["code"], -32602, "{never_known}");
    api.stop().await;
}

#[test]
fn catalog_has_complete_unique_tools_with_resolvable_schemas() {
    let server = CollaborationMcpServer::catalog_only();
    let tools = server.resolved_tools();
    assert_eq!(tools.len(), 24);
    let mut names = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 24);
    assert!(names.contains(&"conversation_resume"));
    assert!(names.contains(&"installation_show"));
    assert!(names.contains(&"conversation_close"));
    assert!(names.contains(&"provider_sessions_list"));
    assert!(names.contains(&"provider_session_inspect"));
    for tool in tools {
        let input = serde_json::to_value(&tool.input_schema).expect("input schema JSON");
        jsonschema::validator_for(&input)
            .unwrap_or_else(|error| panic!("{} input schema: {error}", tool.name));
        let output = serde_json::to_value(
            tool.output_schema
                .as_ref()
                .unwrap_or_else(|| panic!("{} output schema", tool.name)),
        )
        .expect("output schema JSON");
        jsonschema::validator_for(&output)
            .unwrap_or_else(|error| panic!("{} output schema: {error}", tool.name));
    }
}

#[test]
fn catalog_exposes_one_conversation_surface_with_its_composite_tools() {
    // The whole tool set is pinned by the success-schema snapshot; this pins the shape of the
    // conversation surface within it.
    let server = CollaborationMcpServer::catalog_only();
    let actual = server
        .surface
        .router
        .list_all()
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect::<BTreeSet<_>>();
    for composite in [
        "conversation_create",
        "conversation_create_and_prompt",
        "conversation_prompt",
        "events_observe",
    ] {
        assert!(actual.contains(composite), "missing {composite}");
    }
    assert!(
        actual
            .iter()
            .all(|name| !name.starts_with("provider_conversation_")),
        "the MCP catalog must expose one conversation surface"
    );
}

#[test]
fn conversation_catalog_defers_operation_id_requirement_until_route_selection() {
    let server = CollaborationMcpServer::catalog_only();
    let tools = server.surface.router.list_all();
    for name in ["conversation_load", "conversation_prompt"] {
        let tool = tools
            .iter()
            .find(|tool| tool.name == name)
            .expect("common tool");
        let required = tool.input_schema["required"]
            .as_array()
            .expect("required fields");
        assert!(
            !required.contains(&serde_json::json!("operationId")),
            "{name} selects its client before requiring an operation ID"
        );
    }
    let create = tools
        .iter()
        .find(|tool| tool.name == "conversation_create")
        .expect("create tool");
    assert!(
        create.input_schema["required"]
            .as_array()
            .expect("create required fields")
            .contains(&serde_json::json!("operationId"))
    );
    let convenience = tools
        .iter()
        .find(|tool| tool.name == "conversation_create_and_prompt")
        .expect("create and prompt tool");
    assert!(
        !convenience.input_schema["required"]
            .as_array()
            .expect("convenience required fields")
            .contains(&serde_json::json!("promptOperationId"))
    );
}

#[test]
fn tool_schemas_match_known_runtime_defaults_and_conditional_requirements() {
    let server = CollaborationMcpServer::catalog_only();
    let tools = server.resolved_tools();
    let schema_for = |name: &str| {
        tools
            .iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("missing tool {name}"))
            .input_schema
            .as_ref()
            .clone()
            .into()
    };
    fn required_for(schema: &Value) -> &[Value] {
        schema["required"]
            .as_array()
            .unwrap_or_else(|| panic!("schema has no required list: {schema}"))
    }

    let create_schema = schema_for("conversation_create");
    assert!(!required_for(&create_schema).contains(&serde_json::json!("approver")));
    assert!(
        create_schema["allOf"].is_array(),
        "Codex create requirements must be conditional"
    );
    let create_and_prompt_schema = schema_for("conversation_create_and_prompt");
    assert!(
        create_and_prompt_schema["allOf"].is_array(),
        "Codex create-and-prompt requirements must be conditional"
    );
    let service_id = "00000000-0000-4000-8000-000000000001";
    let caller = serde_json::json!({
        "endpoint":{"serviceId":service_id,"endpointId":"codex-local"},"sessionId":"schema-caller"
    });
    let provider_create = serde_json::json!({
        "operationId":"019f0000-0000-7000-8000-000000000201",
        "endpoint":{"serviceId":service_id,"endpointId":"cursor-local"},
        "workingDirectory":"/tmp/project","access":"workspace-write","createdBy":caller
    });
    let create_validator = jsonschema::validator_for(&create_schema).expect("create schema");
    assert!(create_validator.is_valid(&provider_create));
    let human = serde_json::json!({"kind":"human","humanId":"fixture-owner"});
    let mut provider_with_human = provider_create.clone();
    provider_with_human["createdBy"] = human.clone();
    provider_with_human["approver"] = human;
    assert!(create_validator.is_valid(&provider_with_human));
    let _: ConversationCreateToolRequest =
        serde_json::from_value(provider_with_human).expect("typed Human MCP create input");
    let mut provider_with_model = provider_create.clone();
    provider_with_model["model"] = serde_json::json!("gpt-5.6-sol");
    assert!(create_validator.is_valid(&provider_with_model));
    let mut provider_with_effort = provider_create.clone();
    provider_with_effort["effort"] = serde_json::json!("medium");
    assert!(create_validator.is_valid(&provider_with_effort));
    for (field, value) in [
        ("fork", serde_json::json!("source-thread")),
        (
            "rootMessageId",
            serde_json::json!("019f0000-0000-7000-8000-000000000301"),
        ),
    ] {
        let mut provider_with_codex_only_field = provider_create.clone();
        provider_with_codex_only_field[field] = value;
        assert!(
            !create_validator.is_valid(&provider_with_codex_only_field),
            "provider create schema must reject {field}"
        );
    }
    let mut empty_codex_create = provider_create.clone();
    empty_codex_create["endpoint"]["endpointId"] = serde_json::json!("codex-local");
    assert!(!create_validator.is_valid(&empty_codex_create));
    empty_codex_create["model"] = serde_json::json!("gpt-5.6-sol");
    empty_codex_create["effort"] = serde_json::json!("medium");
    assert!(create_validator.is_valid(&empty_codex_create));

    let create_and_prompt_validator =
        jsonschema::validator_for(&create_and_prompt_schema).expect("create-and-prompt schema");
    let provider_create_and_prompt = serde_json::json!({
        "create":provider_create,
        "message":{"kind":"humanUser","text":"hello"}
    });
    assert!(create_and_prompt_validator.is_valid(&provider_create_and_prompt));
    let mut provider_create_and_prompt_with_effort = provider_create_and_prompt;
    provider_create_and_prompt_with_effort["create"]["effort"] = serde_json::json!("medium");
    assert!(create_and_prompt_validator.is_valid(&provider_create_and_prompt_with_effort));
    for (field, value) in [
        ("fork", serde_json::json!("source-thread")),
        (
            "rootMessageId",
            serde_json::json!("019f0000-0000-7000-8000-000000000302"),
        ),
    ] {
        let mut provider_create_and_prompt = serde_json::json!({
            "create":provider_create,
            "message":{"kind":"humanUser","text":"hello"}
        });
        provider_create_and_prompt["create"][field] = value;
        assert!(
            !create_and_prompt_validator.is_valid(&provider_create_and_prompt),
            "provider create-and-prompt schema must reject {field}"
        );
    }

    let descriptions = tools
        .iter()
        .map(|tool| {
            (
                tool.name.as_ref(),
                tool.description.as_deref().unwrap_or_default(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for tool in ["conversation_create", "conversation_create_and_prompt"] {
        let description = descriptions
            .get(tool)
            .expect("conversation create description");
        assert!(
            description.contains("required for Codex endpoints"),
            "{description}"
        );
        assert!(
            description.contains("rejected for provider endpoints"),
            "{description}"
        );
        assert!(description.contains("rootMessageId"), "{description}");
        assert!(description.contains("fork"), "{description}");
    }
}

#[test]
fn provider_create_schemas_accept_settings_and_keep_endpoint_specific_refusals() {
    let server = CollaborationMcpServer::catalog_only();
    for tool_name in ["conversation_create", "conversation_create_and_prompt"] {
        let tool = server
            .resolved_tools()
            .into_iter()
            .find(|tool| tool.name == tool_name)
            .expect("conversation tool");
        let schema = Value::Object(tool.input_schema.as_ref().clone());
        let validator = jsonschema::validator_for(&schema).expect("advertised input schema");
        let request = |create: Value| {
            if tool_name == "conversation_create" {
                create
            } else {
                serde_json::json!({"create":create,"message":{"kind":"humanUser","text":"hello"}})
            }
        };
        for (endpoint, model) in [("claude-local", "haiku"), ("cursor-local", "grok-4.7")] {
            let create = serde_json::json!({
                "operationId":"019f0000-0000-7000-8000-000000000401",
                "endpoint":{"serviceId":FIXTURE_SERVICE_ID,"endpointId":endpoint},
                "workingDirectory":"/tmp/project","access":"write-restricted",
                "createdBy":{"kind":"human","humanId":"fixture-owner"},
                "model":model,"effort":"low"
            });
            assert!(
                validator.is_valid(&request(create.clone())),
                "{tool_name}: {endpoint} must accept model and effort"
            );
            for omitted in ["model", "effort"] {
                let mut optional = create.clone();
                optional
                    .as_object_mut()
                    .expect("create object")
                    .remove(omitted);
                assert!(
                    validator.is_valid(&request(optional)),
                    "{tool_name}: {endpoint} may omit {omitted}"
                );
            }
            for (field, value) in [
                ("fork", serde_json::json!("source-thread")),
                (
                    "rootMessageId",
                    serde_json::json!("019f0000-0000-7000-8000-000000000402"),
                ),
            ] {
                let mut refused = create.clone();
                refused[field] = value;
                assert!(
                    !validator.is_valid(&request(refused)),
                    "{tool_name}: {endpoint} must reject {field}"
                );
            }
        }
        let codex = serde_json::json!({
            "operationId":"019f0000-0000-7000-8000-000000000403",
            "endpoint":{"serviceId":FIXTURE_SERVICE_ID,"endpointId":"codex-local"},
            "workingDirectory":"/tmp/project","access":"write-restricted",
            "createdBy":{"kind":"human","humanId":"fixture-owner"},
            "model":"gpt-6-luna","effort":"low"
        });
        assert!(
            validator.is_valid(&request(codex.clone())),
            "{tool_name}: complete Codex declaration"
        );
        for omitted in ["model", "effort"] {
            let mut missing = codex.clone();
            missing
                .as_object_mut()
                .expect("create object")
                .remove(omitted);
            assert!(
                !validator.is_valid(&request(missing)),
                "{tool_name}: Codex must require {omitted}"
            );
        }
    }
}

#[test]
fn sdk_only_operations_are_advertised_by_both_cli_and_mcp_catalogs() {
    let help = agent_collaboration::command_help();
    for (tool, cli_command) in [
        ("conversation_create", "conversation create"),
        ("conversation_load", "conversation load"),
        ("conversation_create_and_prompt", "conversation prompt"),
        ("conversation_prompt", "conversation prompt"),
        ("conversation_cancel", "conversation cancel"),
        ("events_observe", "events observe"),
    ] {
        assert!(help.contains(cli_command), "CLI adapter missing for {tool}");
    }
}

#[test]
fn conversation_create_tool_preserves_created_and_pending_operation_identity() {
    let operation_id = OperationId::generate();
    let target = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
        "sessionId":"created-thread"
    }))
    .expect("target");
    let created = conversation_create_tool_result(
        Ok(ConversationCreateOutcome::Created {
            operation_id: operation_id.clone(),
            target,
            effective_settings: None,
            permission_differences: Vec::new(),
        }),
        operation_id.clone(),
    );
    assert_eq!(created.is_error, Some(false));
    assert_eq!(
        created.structured_content.as_ref().unwrap()["kind"],
        "created"
    );
    assert_eq!(
        created.structured_content.as_ref().unwrap()["operationId"],
        serde_json::json!(operation_id)
    );
    assert_eq!(
        created.structured_content.as_ref().unwrap()["target"]["sessionId"],
        "created-thread"
    );

    let pending = conversation_create_tool_result(
        Ok(ConversationCreateOutcome::Pending {
            operation_id: operation_id.clone(),
        }),
        operation_id.clone(),
    );
    assert_eq!(pending.is_error, Some(false));
    assert_eq!(
        pending.structured_content.as_ref().unwrap()["kind"],
        "pending"
    );
    assert_eq!(
        pending.structured_content.as_ref().unwrap()["operationId"],
        serde_json::json!(operation_id)
    );
    assert!(
        pending
            .structured_content
            .as_ref()
            .unwrap()
            .get("target")
            .is_none()
    );
}

#[test]
fn conversation_create_tool_reports_endpoint_named_unsupported_input() {
    let operation_id = OperationId::generate();
    let endpoint = serde_json::from_value(serde_json::json!({
        "serviceId":"00000000-0000-4000-8000-000000000001", "endpointId":"cursor-local"
    }))
    .expect("endpoint");
    let response = conversation_create_tool_result(
        Err(
            collaboration_client::ConversationClientError::UnsupportedInput {
                endpoint,
                field: "model",
                fix: "omit model for cursor-local".to_owned(),
            },
        ),
        operation_id.clone(),
    );
    assert_eq!(response.is_error, Some(true));
    let content = response.structured_content.expect("structured error");
    assert_eq!(content["kind"], "unsupportedCapability");
    assert_eq!(content["operationId"], serde_json::json!(operation_id));
    assert_eq!(content["endpoint"]["endpointId"], "cursor-local");
    assert_eq!(content["field"], "model");
    assert_eq!(content["fix"], "omit model for cursor-local");
}

#[test]
fn representative_catalog_descriptions_explain_operation_specific_behavior() {
    let server = CollaborationMcpServer::catalog_only();
    let descriptions = server
        .resolved_tools()
        .into_iter()
        .map(|tool| {
            (
                tool.name.into_owned(),
                tool.description.unwrap_or_default().into_owned(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for description in descriptions.values() {
        assert!(!description.contains("Calls the existing typed"));
        assert!(!description.contains("Undocumented collaboration operation"));
    }
    for (name, required_phrases) in [
        ("endpoints_list", &["Read-only"][..]),
        (
            "conversation_create_and_prompt",
            &["advertised client", "completed turn", "peer reply"][..],
        ),
    ] {
        let description = descriptions
            .get(name)
            .unwrap_or_else(|| panic!("missing tool {name}"));
        for phrase in required_phrases {
            assert!(
                description.contains(phrase),
                "{name} description missing {phrase:?}: {description}"
            );
        }
    }
}
