//! N3 node taps for the item rows of the conversion catalog: ItemStarted (other
//! kinds), ItemUpdated and ItemCompleted.
use super::node_tap_tests::{
    Rig, agent_message, base_state, expectation, frame, in_turn, item, item_completed_frame,
    item_started_frame, plan, started, tap_event, text_delta_frame, thought, tool, updated,
};
use super::*;
use session_event_model::{SessionItemKind, ToolCallStatus};

const ROW_STARTED: &str = "N3 catalog: ItemStarted, other kinds";
const ROW_UPDATED: &str = "N3 catalog: ItemUpdated";
const ROW_COMPLETED: &str = "N3 catalog: ItemCompleted";

#[test]
fn node_tap_item_started_agent_message_with_text_adds_delta() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    let row = expectation(
        &[
            item_started_frame(
                "turn-1",
                json!({"type":"agentMessage","id":"m","text":"hello"}),
            ),
            text_delta_frame("item/agentMessage/delta", "turn-1", "m", "hello"),
        ],
        in_turn()
            .field("stored_items", "m:agentMessage")
            .field("text_already_sent", "m=hello"),
    );
    tap_event(
        &mut rig,
        "item_started_agent_message_with_text",
        ROW_STARTED,
        started(agent_message("hello")),
        &row,
    );
}

#[test]
fn node_tap_item_started_agent_message_without_text_has_no_delta() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    let row = expectation(
        &[item_started_frame(
            "turn-1",
            json!({"type":"agentMessage","id":"m","text":""}),
        )],
        in_turn()
            .field("stored_items", "m:agentMessage")
            .field("text_already_sent", "m="),
    );
    tap_event(
        &mut rig,
        "item_started_agent_message_empty",
        ROW_STARTED,
        started(agent_message("")),
        &row,
    );
}

#[test]
fn node_tap_item_started_reasoning_with_text_adds_summary_delta() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    let row = expectation(
        &[
            item_started_frame(
                "turn-1",
                json!({"type":"reasoning","id":"t","summary":["think"],"content":[]}),
            ),
            text_delta_frame("item/reasoning/summaryTextDelta", "turn-1", "t", "think"),
        ],
        in_turn()
            .field("stored_items", "t:reasoning")
            .field("text_already_sent", "t=think"),
    );
    tap_event(
        &mut rig,
        "item_started_reasoning_with_text",
        ROW_STARTED,
        started(thought("think")),
        &row,
    );
}

fn started_without_delta_case(
    case_name: &str,
    session_item: SessionItem,
    thread_item: Value,
    stored: &str,
    text_sent: &str,
) {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    let row = expectation(
        &[item_started_frame("turn-1", thread_item)],
        in_turn()
            .field("stored_items", stored)
            .field("text_already_sent", text_sent),
    );
    tap_event(
        &mut rig,
        case_name,
        ROW_STARTED,
        started(session_item),
        &row,
    );
}

#[test]
fn node_tap_item_started_tool_call_has_no_delta() {
    started_without_delta_case(
        "item_started_tool_call",
        tool(ToolCallStatus::InProgress, Some("reading")),
        json!({"type":"mcpToolCall","id":"tool","server":"provider","tool":"read","status":"inProgress","arguments":{},"result":null,"error":null}),
        "tool:mcpToolCall",
        "none",
    );
}

#[test]
fn node_tap_item_started_notice_is_shown_as_agent_message() {
    started_without_delta_case(
        "item_started_notice",
        item("n", SessionItemKind::Notice, Some("reconnected")),
        json!({"type":"agentMessage","id":"n","text":"reconnected"}),
        "n:agentMessage",
        "n=reconnected",
    );
}

#[test]
fn node_tap_item_started_unknown_is_shown_as_agent_message() {
    started_without_delta_case(
        "item_started_unknown",
        item(
            "k",
            SessionItemKind::Unknown {
                source_kind: "future".into(),
            },
            Some("later"),
        ),
        json!({"type":"agentMessage","id":"k","text":"later"}),
        "k:agentMessage",
        "k=later",
    );
}

#[test]
fn node_tap_item_started_context_compaction_has_no_delta() {
    started_without_delta_case(
        "item_started_context_compaction",
        item(
            "c",
            SessionItemKind::ToolCall {
                tool_kind: "contextCompaction".into(),
                status: ToolCallStatus::Completed,
            },
            None,
        ),
        json!({"type":"contextCompaction","id":"c"}),
        "c:contextCompaction",
        "none",
    );
}

#[test]
fn node_tap_item_started_user_message_is_stamped_with_client_id() {
    let mut rig = Rig::new();
    let input_id = rig.register_user_input("client-1");
    rig.setup(SessionEvent::TurnStarted {
        turn_id: "turn-1".into(),
        input_id,
    });
    let row = expectation(
        &[item_started_frame(
            "turn-1",
            json!({"type":"userMessage","id":"u","content":[{"type":"text","text":"hello"}],"clientId":"client-1"}),
        )],
        in_turn().field("stored_items", "u:userMessage").field(
            "receipts",
            "outstanding=[client-1@u] accepted_for_next_item=none",
        ),
    );
    tap_event(
        &mut rig,
        "item_started_user_message_stamped",
        ROW_STARTED,
        started(item("u", SessionItemKind::UserMessage, Some("hello"))),
        &row,
    );
}

#[test]
fn node_tap_item_updated_agent_message_sends_suffix_only() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    rig.setup(started(agent_message("Hel")));
    let row = expectation(
        &[text_delta_frame(
            "item/agentMessage/delta",
            "turn-1",
            "m",
            "lo",
        )],
        in_turn()
            .field("stored_items", "m:agentMessage")
            .field("text_already_sent", "m=Hello"),
    );
    tap_event(
        &mut rig,
        "item_updated_agent_message_suffix",
        ROW_UPDATED,
        updated(agent_message("Hello")),
        &row,
    );
}

#[test]
fn node_tap_item_updated_agent_message_not_a_prefix_sends_nothing() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    rig.setup(started(agent_message("Hello")));
    // The catalog's state column says "item stored": the stored copy follows the new text.
    let row = expectation(
        &[],
        in_turn()
            .field("stored_items", "m:agentMessage")
            .field("text_already_sent", "m=Goodbye"),
    );
    tap_event(
        &mut rig,
        "item_updated_agent_message_not_prefix",
        ROW_UPDATED,
        updated(agent_message("Goodbye")),
        &row,
    );
}

#[test]
fn node_tap_item_updated_reasoning_sends_suffix_only() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    rig.setup(started(thought("Fir")));
    let row = expectation(
        &[text_delta_frame(
            "item/reasoning/summaryTextDelta",
            "turn-1",
            "t",
            "st",
        )],
        in_turn()
            .field("stored_items", "t:reasoning")
            .field("text_already_sent", "t=First"),
    );
    tap_event(
        &mut rig,
        "item_updated_reasoning_suffix",
        ROW_UPDATED,
        updated(thought("First")),
        &row,
    );
}

#[test]
fn node_tap_item_updated_tool_call_with_text_reports_progress() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    rig.setup(started(tool(ToolCallStatus::InProgress, None)));
    let row = expectation(
        &[frame(
            "item/mcpToolCall/progress",
            json!({"turnId":"turn-1","itemId":"tool","message":"halfway"}),
        )],
        in_turn().field("stored_items", "tool:mcpToolCall"),
    );
    tap_event(
        &mut rig,
        "item_updated_tool_call_progress",
        ROW_UPDATED,
        updated(tool(ToolCallStatus::InProgress, Some("halfway"))),
        &row,
    );
}

#[test]
fn node_tap_item_updated_tool_call_without_text_sends_nothing() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    rig.setup(started(tool(ToolCallStatus::InProgress, None)));
    let row = expectation(&[], in_turn().field("stored_items", "tool:mcpToolCall"));
    tap_event(
        &mut rig,
        "item_updated_tool_call_no_text",
        ROW_UPDATED,
        updated(tool(ToolCallStatus::Completed, None)),
        &row,
    );
}

#[test]
fn node_tap_item_updated_other_kinds_send_nothing() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    rig.setup(started(item("n", SessionItemKind::Notice, Some("a"))));
    let row = expectation(
        &[],
        in_turn()
            .field("stored_items", "n:agentMessage")
            .field("text_already_sent", "n=ab"),
    );
    tap_event(
        &mut rig,
        "item_updated_notice",
        ROW_UPDATED,
        updated(item("n", SessionItemKind::Notice, Some("ab"))),
        &row,
    );
}

#[test]
fn node_tap_item_completed_known_item_in_turn() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    rig.setup(started(agent_message("done")));
    let row = expectation(
        &[item_completed_frame(
            "turn-1",
            json!({"type":"agentMessage","id":"m","text":"done"}),
        )],
        in_turn(),
    );
    tap_event(
        &mut rig,
        "item_completed_known",
        ROW_COMPLETED,
        SessionEvent::ItemCompleted {
            item_id: "m".into(),
        },
        &row,
    );
}

#[test]
fn node_tap_item_completed_unknown_item_sends_nothing() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    let row = expectation(&[], in_turn());
    tap_event(
        &mut rig,
        "item_completed_unknown",
        ROW_COMPLETED,
        SessionEvent::ItemCompleted {
            item_id: "never-started".into(),
        },
        &row,
    );
}

#[test]
fn node_tap_item_completed_plan_sends_nothing() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    rig.setup(started(plan("Do X")));
    let row = expectation(&[], in_turn());
    tap_event(
        &mut rig,
        "item_completed_plan",
        ROW_COMPLETED,
        SessionEvent::ItemCompleted {
            item_id: "p".into(),
        },
        &row,
    );
}

#[test]
fn node_tap_item_completed_outside_turn_sends_nothing() {
    let mut rig = Rig::new();
    rig.setup(started(agent_message("early")));
    let row = expectation(&[], base_state());
    tap_event(
        &mut rig,
        "item_completed_outside_turn",
        ROW_COMPLETED,
        SessionEvent::ItemCompleted {
            item_id: "m".into(),
        },
        &row,
    );
}
