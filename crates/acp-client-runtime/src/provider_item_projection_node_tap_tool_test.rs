//! N6 tap cases for tool calls, tool call updates, plans and the current mode update.

use super::*;

// --- Tool calls ------------------------------------------------------------

#[test]
fn node_tap_tool_call_new_id_closes_open_text_then_starts() {
    let mut harness = Harness::new();
    harness.prime(&agent_message("hello", "m1"));
    let case = harness.tap_update(
        "tool_call_new_id",
        "N6 catalog: Tool call: closes open text, then 1 Started (new id)",
        "session/update:tool_call",
        &tool_call(
            "t1",
            "Read file",
            ToolKind::Read,
            AcpToolCallStatus::Pending,
        ),
    );
    let row = expectation(
        vec![
            completed("ItemCompleted(AgentMessage)", "m1"),
            started(
                "ItemStarted(ToolCall(read,Pending))",
                "t1",
                tool_kind_json("read", "pending"),
                Some("Read file"),
            ),
        ],
        state(
            NO_STATE,
            "t1=ToolCall(read,Pending) id=t1 text=\"Read file\"",
            "t1",
            NO_STATE,
        ),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_tool_call_known_id_updates() {
    let mut harness = Harness::new();
    harness.prime(&tool_call(
        "t1",
        "Read file",
        ToolKind::Read,
        AcpToolCallStatus::Pending,
    ));
    let case = harness.tap_update(
        "tool_call_known_id",
        "N6 catalog: Tool call: 1 Updated (known id)",
        "session/update:tool_call",
        &tool_call(
            "t1",
            "Read file",
            ToolKind::Read,
            AcpToolCallStatus::InProgress,
        ),
    );
    let row = expectation(
        vec![updated(
            "ItemUpdated(ToolCall(read,InProgress))",
            "t1",
            tool_kind_json("read", "inProgress"),
            Some("Read file"),
        )],
        state(
            NO_STATE,
            "t1=ToolCall(read,InProgress) id=t1 text=\"Read file\"",
            "t1",
            NO_STATE,
        ),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_tool_call_update_unknown_id_starts_as_other_pending() {
    let mut harness = Harness::new();
    harness.prime(&agent_message("hello", "m1"));
    let case = harness.tap_update(
        "tool_call_update_unknown_id",
        "N6 catalog: Tool call update: 1 Started (unknown id); unknown id starts as other / pending",
        "session/update:tool_call_update",
        &tool_update("t9", ToolCallUpdateFields::new()),
    );
    let row = expectation(
        vec![
            completed("ItemCompleted(AgentMessage)", "m1"),
            started(
                "ItemStarted(ToolCall(other,Pending))",
                "t9",
                tool_kind_json("other", "pending"),
                None,
            ),
        ],
        state(
            NO_STATE,
            "t9=ToolCall(other,Pending) id=t9 text=\"<none>\"",
            "t9",
            NO_STATE,
        ),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_tool_call_update_known_id_updates() {
    let mut harness = Harness::new();
    harness.prime(&tool_call(
        "t1",
        "Read file",
        ToolKind::Read,
        AcpToolCallStatus::Pending,
    ));
    let case = harness.tap_update(
        "tool_call_update_known_id",
        "N6 catalog: Tool call update: 1 Updated (known id)",
        "session/update:tool_call_update",
        &tool_update(
            "t1",
            ToolCallUpdateFields::new()
                .status(AcpToolCallStatus::Completed)
                .title("Read file done".to_owned()),
        ),
    );
    let row = expectation(
        vec![updated(
            "ItemUpdated(ToolCall(read,Completed))",
            "t1",
            tool_kind_json("read", "completed"),
            Some("Read file done"),
        )],
        state(
            NO_STATE,
            "t1=ToolCall(read,Completed) id=t1 text=\"Read file done\"",
            "t1",
            NO_STATE,
        ),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_tool_call_update_keeps_context_compaction_kind() {
    let mut harness = harness_with_completed_compaction();
    harness.projection.promote_compaction().unwrap();
    let _ = harness.drain();
    let case = harness.tap_update(
        "tool_call_update_keeps_compaction_kind",
        "N6 catalog: Tool call update: contextCompaction kind is kept",
        "session/update:tool_call_update",
        &tool_update(
            "compact-1",
            ToolCallUpdateFields::new().kind(ToolKind::Edit),
        ),
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

// --- Plan ------------------------------------------------------------------

#[test]
fn node_tap_plan_first_closes_open_text_then_starts() {
    let mut harness = Harness::new();
    harness.prime(&agent_message("hello", "m1"));
    let case = harness.tap_update(
        "plan_first",
        "N6 catalog: Plan: closes open text, then 1 Started (first)",
        "session/update:plan",
        &plan(&[("step a", PlanEntryStatus::Pending)]),
    );
    let row = expectation(
        vec![
            completed("ItemCompleted(AgentMessage)", "m1"),
            started(
                "ItemStarted(Plan)",
                VOLATILE_ID,
                plain_kind("plan"),
                Some("- Pending: step a"),
            ),
        ],
        state(
            NO_STATE,
            NO_STATE,
            NO_STATE,
            "Plan text=\"- Pending: step a\"",
        ),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_plan_subsequent_updates_with_one_stable_id() {
    let mut harness = Harness::new();
    harness
        .projection
        .observe(&plan(&[("step a", PlanEntryStatus::Pending)]), None)
        .unwrap();
    let first_id = match harness.sink.recorded.lock().unwrap().first() {
        Some(Recorded::Event(SessionEvent::ItemStarted { item })) => item.item_id.clone(),
        _ => panic!("first plan did not start an item"),
    };
    let _ = harness.drain();
    let case = harness.tap_update(
        "plan_subsequent",
        "N6 catalog: Plan: 1 Updated (subsequent); one stable id per turn",
        "session/update:plan",
        &plan(&[("step a", PlanEntryStatus::Completed)]),
    );
    assert_eq!(
        harness
            .projection
            .plan_item
            .as_ref()
            .map(|item| item.item_id.as_str()),
        Some(first_id.as_str()),
        "plan keeps one stable id"
    );
    let row = expectation(
        vec![updated(
            "ItemUpdated(Plan)",
            VOLATILE_ID,
            plain_kind("plan"),
            Some("- Completed: step a"),
        )],
        state(
            NO_STATE,
            NO_STATE,
            NO_STATE,
            "Plan text=\"- Completed: step a\"",
        ),
    );
    case.finish(&row).assert_settled();
}

// --- Mode ------------------------------------------------------------------

#[test]
fn node_tap_current_mode_update_without_open_text() {
    let mut harness = Harness::new();
    let case = harness.tap_update(
        "current_mode_update",
        "N6 catalog: Current mode update: Started + Completed: 2 events; ModeChange { mode id }",
        "session/update:current_mode_update",
        &SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new("plan")),
    );
    let row = expectation(
        vec![
            started(
                "ItemStarted(ModeChange id=plan)",
                VOLATILE_ID,
                plain_kind("modeChange"),
                Some("plan"),
            ),
            completed("ItemCompleted(ModeChange)", VOLATILE_ID),
        ],
        state(NO_STATE, NO_STATE, NO_STATE, NO_STATE),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_current_mode_update_closes_open_text() {
    let mut harness = Harness::new();
    harness.prime(&agent_message("hello", "m1"));
    let case = harness.tap_update(
        "current_mode_update_closes_open_text",
        "N6 catalog: Current mode update: 0 or 1 ItemCompleted for open text, then Started + Completed: 2-3 events",
        "session/update:current_mode_update",
        &SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new("plan")),
    );
    let row = expectation(
        vec![
            completed("ItemCompleted(AgentMessage)", "m1"),
            started(
                "ItemStarted(ModeChange id=plan)",
                VOLATILE_ID,
                plain_kind("modeChange"),
                Some("plan"),
            ),
            completed("ItemCompleted(ModeChange)", VOLATILE_ID),
        ],
        state(NO_STATE, NO_STATE, NO_STATE, NO_STATE),
    );
    case.finish(&row).assert_settled();
}
