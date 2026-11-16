//! N6 tap cases for metadata updates, the decode-level unknown kind and other transitions.

use super::*;

// --- Typed-payload rows (D2) -----------------------------------------------

/// The typed row's expected event: the catalog's typed payload in place of today's JSON text.
fn typed(label: &str, kind: serde_json::Value, payload: serde_json::Value) -> ExpectedEvent {
    (
        label.to_owned(),
        json!({
            "kind": "itemStarted",
            "item": { "itemId": VOLATILE_ID, "kind": kind, "payload": payload }
        }),
    )
}

/// The whole skill-view publication the provider seam reports: an available view with no
/// skills and no errors, in its full `Debug` text.
fn skill_view_published() -> ExpectedEvent {
    let view = SkillView::Available {
        skills: Vec::new(),
        errors: Vec::new(),
    };
    (
        "SkillViewPublished(Available)".to_owned(),
        json!({ "debug": format!("{view:?}") }),
    )
}

fn config_option_update() -> SessionUpdate {
    let update = serde_json::from_value::<ConfigOptionUpdate>(serde_json::json!({
        "configOptions": [{
            "id": "model", "name": "Model", "type": "select", "currentValue": "a",
            "options": [{ "value": "a", "name": "A" }]
        }]
    }))
    .unwrap();
    assert_eq!(update.config_options.len(), 1, "fixture decodes one option");
    SessionUpdate::ConfigOptionUpdate(update)
}

#[test]
fn node_tap_config_option_update_carries_typed_options() {
    let mut harness = Harness::new();
    let mut case = harness.tap_update(
        "config_option_update",
        "N6 catalog: Config option update; change: ConfigChange carries typed config options, not a JSON string",
        "session/update:config_option_update",
        &config_option_update(),
    );
    let none = state(NO_STATE, NO_STATE, NO_STATE, NO_STATE);
    case.current_defect(expectation(
        vec![
            started("ItemStarted(ConfigChange payload=jsonText)", VOLATILE_ID, plain_kind("configChange"), Some("[{\"id\":\"model\",\"name\":\"Model\",\"type\":\"select\",\"currentValue\":\"a\",\"options\":[{\"value\":\"a\",\"name\":\"A\"}]}]")),
            completed("ItemCompleted(ConfigChange)", VOLATILE_ID),
        ],
        none.clone(),
    ));
    let row = expectation(
        vec![
            typed(
                "ItemStarted(ConfigChange payload=typed)",
                plain_kind("configChange"),
                json!([{ "id": "model", "name": "Model", "type": "select", "currentValue": "a",
                    "options": [{ "value": "a", "name": "A" }] }]),
            ),
            completed("ItemCompleted(ConfigChange)", VOLATILE_ID),
        ],
        none,
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_session_info_update_carries_typed_fields() {
    let mut harness = Harness::new();
    let mut case = harness.tap_update(
        "session_info_update",
        "N6 catalog: Session info update; change: SessionInfo carries typed fields",
        "session/update:session_info_update",
        &SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title("A title".to_owned())),
    );
    let none = state(NO_STATE, NO_STATE, NO_STATE, NO_STATE);
    case.current_defect(expectation(
        vec![
            started(
                "ItemStarted(SessionInfo payload=jsonText)",
                VOLATILE_ID,
                plain_kind("sessionInfo"),
                Some("{\"title\":\"A title\"}"),
            ),
            completed("ItemCompleted(SessionInfo)", VOLATILE_ID),
        ],
        none.clone(),
    ));
    let row = expectation(
        vec![
            typed(
                "ItemStarted(SessionInfo payload=typed)",
                plain_kind("sessionInfo"),
                json!({ "title": "A title" }),
            ),
            completed("ItemCompleted(SessionInfo)", VOLATILE_ID),
        ],
        none,
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_usage_update_carries_typed_used_and_size() {
    let mut harness = Harness::new();
    let mut case = harness.tap_update(
        "usage_update",
        "N6 catalog: Usage update; change: Usage { used, size } typed",
        "session/update:usage_update",
        &SessionUpdate::UsageUpdate(UsageUpdate::new(120, 1000)),
    );
    let none = state(NO_STATE, NO_STATE, NO_STATE, NO_STATE);
    case.current_defect(expectation(
        vec![
            started(
                "ItemStarted(Usage payload=jsonText)",
                VOLATILE_ID,
                plain_kind("usage"),
                Some("{\"used\":120,\"size\":1000}"),
            ),
            completed("ItemCompleted(Usage)", VOLATILE_ID),
        ],
        none.clone(),
    ));
    let row = expectation(
        vec![
            typed(
                "ItemStarted(Usage payload=typed)",
                plain_kind("usage"),
                json!({ "used": 120, "size": 1000 }),
            ),
            completed("ItemCompleted(Usage)", VOLATILE_ID),
        ],
        none,
    );
    case.finish(&row).assert_settled();
}

fn commands_update() -> SessionUpdate {
    SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(vec![
        AvailableCommand::new("compact", "Compact the context"),
    ]))
}

#[test]
fn node_tap_available_commands_update_is_a_typed_command_catalog() {
    let mut harness = Harness::new();
    let mut case = harness.tap_update(
        "available_commands_update",
        "N6 catalog: Available commands update; change: CommandCatalog typed (today a Notice whose text is JSON)",
        "session/update:available_commands_update",
        &commands_update(),
    );
    let none = state(NO_STATE, NO_STATE, NO_STATE, NO_STATE);
    case.current_defect(expectation(
        vec![
            started("ItemStarted(Notice payload=jsonText)", VOLATILE_ID, plain_kind("notice"), Some("{\"availableCommands\":[{\"name\":\"compact\",\"description\":\"Compact the context\"}]}")),
            completed("ItemCompleted(Notice)", VOLATILE_ID),
        ],
        none.clone(),
    ));
    let row = expectation(
        vec![
            typed(
                "ItemStarted(CommandCatalog payload=typed)",
                plain_kind("commandCatalog"),
                json!([{ "name": "compact", "description": "Compact the context" }]),
            ),
            completed("ItemCompleted(CommandCatalog)", VOLATILE_ID),
        ],
        none,
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_available_commands_update_with_skill_view() {
    let mut harness = Harness::with_behavior(ScriptedBehavior {
        skill_view_on_commands: true,
    });
    let mut case = harness.tap_update(
        "available_commands_update_with_skill_view",
        "N6 catalog: Available commands update: same events, plus the skill-view update (unchanged)",
        "session/update:available_commands_update",
        &commands_update(),
    );
    let none = state(NO_STATE, NO_STATE, NO_STATE, NO_STATE);
    // Today the skill view is published first, then the notice pair.
    case.current_defect(expectation(
        vec![
            skill_view_published(),
            started("ItemStarted(Notice payload=jsonText)", VOLATILE_ID, plain_kind("notice"), Some("{\"availableCommands\":[{\"name\":\"compact\",\"description\":\"Compact the context\"}]}")),
            completed("ItemCompleted(Notice)", VOLATILE_ID),
        ],
        none.clone(),
    ));
    let row = expectation(
        vec![
            skill_view_published(),
            typed(
                "ItemStarted(CommandCatalog payload=typed)",
                plain_kind("commandCatalog"),
                json!([{ "name": "compact", "description": "Compact the context" }]),
            ),
            completed("ItemCompleted(CommandCatalog)", VOLATILE_ID),
        ],
        none,
    );
    case.finish(&row).assert_settled();
}

// --- Decode-level unknown kind and other transitions -------------------------

#[test]
fn node_tap_unknown_update_kind_without_open_text() {
    let mut harness = Harness::new();
    let case = harness.tap(
        "unknown_update_kind",
        "N6 catalog: Unknown update kind (decode level): Started + Completed: 2 events; Unknown { source kind }",
        "session/update:futureKind",
        |projection| projection.observe_unknown("futureKind", Some("raw payload")),
    );
    let row = expectation(
        vec![
            started(
                "ItemStarted(Unknown(futureKind))",
                VOLATILE_ID,
                json!({ "kind": "unknown", "source_kind": "futureKind" }),
                Some("raw payload"),
            ),
            completed("ItemCompleted(Unknown(futureKind))", VOLATILE_ID),
        ],
        state(NO_STATE, NO_STATE, NO_STATE, NO_STATE),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_unknown_update_kind_closes_open_text() {
    let mut harness = Harness::new();
    harness.prime(&agent_message("hello", "m1"));
    let case = harness.tap(
        "unknown_update_kind_closes_open_text",
        "N6 catalog: Unknown update kind: 0 or 1 ItemCompleted for open text, then Started + Completed",
        "session/update:futureKind",
        |projection| projection.observe_unknown("futureKind", None),
    );
    let row = expectation(
        vec![
            completed("ItemCompleted(AgentMessage)", "m1"),
            started(
                "ItemStarted(Unknown(futureKind))",
                VOLATILE_ID,
                json!({ "kind": "unknown", "source_kind": "futureKind" }),
                Some("Unrecognized agent update"),
            ),
            completed("ItemCompleted(Unknown(futureKind))", VOLATILE_ID),
        ],
        state(NO_STATE, NO_STATE, NO_STATE, NO_STATE),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_turn_boundary_clears_turn_cursors_but_keeps_tool_items() {
    let mut harness = Harness::new();
    harness.prime(&tool_call(
        "t1",
        "Read file",
        ToolKind::Read,
        AcpToolCallStatus::Completed,
    ));
    harness.prime(&plan(&[("step a", PlanEntryStatus::Pending)]));
    harness.prime(&agent_message("hello", "m1"));
    let case = harness.tap("turn_boundary_clears_cursors", "N6 catalog: Other transitions: Turn start and end clear the turn-local cursors but keep the tool items", "turn boundary", |projection| {
        projection.discard_turn_local_cursors();
        Ok(())
    });
    let row = expectation(
        vec![],
        state(
            NO_STATE,
            "t1=ToolCall(read,Completed) id=t1 text=\"Read file\"",
            NO_STATE,
            NO_STATE,
        ),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_finish_completes_text_then_tools_then_plan() {
    let mut harness = Harness::new();
    harness.prime(&plan(&[("step a", PlanEntryStatus::Pending)]));
    harness.prime(&tool_call(
        "t1",
        "Read file",
        ToolKind::Read,
        AcpToolCallStatus::Pending,
    ));
    harness.prime(&tool_call(
        "t2",
        "Edit file",
        ToolKind::Edit,
        AcpToolCallStatus::Pending,
    ));
    harness.prime(&agent_message("hello", "m1"));
    let case = harness.tap(
        "finish_order",
        "N6 catalog: Other transitions: finish() completes the text, then the tools, then the plan",
        "turn end",
        |projection| projection.finish(),
    );
    let row = expectation(
        vec![
            completed("ItemCompleted(AgentMessage)", "m1"),
            completed("ItemCompleted(ToolCall)", "t1"),
            completed("ItemCompleted(ToolCall)", "t2"),
            completed("ItemCompleted(Plan)", VOLATILE_ID),
        ],
        state(
            NO_STATE,
            "t1=ToolCall(read,Pending) id=t1 text=\"Read file\"; t2=ToolCall(edit,Pending) id=t2 text=\"Edit file\"",
            NO_STATE,
            NO_STATE,
        ),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_compaction_promotion_emits_one_update() {
    let mut harness = harness_with_completed_compaction();
    let case = harness.tap(
        "compaction_promotion",
        "N6 catalog: Other transitions: Compaction promotion emits one Updated",
        "turn end (compaction hint)",
        |projection| projection.promote_compaction(),
    );
    let row = expectation(
        vec![updated(
            "ItemUpdated(ToolCall(contextCompaction,Completed))",
            "compact-1",
            tool_kind_json("contextCompaction", "completed"),
            Some("Compact"),
        )],
        state(
            NO_STATE,
            "compact-1=ToolCall(contextCompaction,Completed) id=compact-1 text=\"Compact\"",
            "compact-1",
            NO_STATE,
        ),
    );
    case.finish(&row).assert_settled();
}
