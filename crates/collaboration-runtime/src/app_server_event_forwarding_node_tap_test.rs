//! N3 node taps: one session event per catalog row, driven through the real
//! `AppServerEventForwarding`, compared with the Program Design's
//! "N3 · Session event → Codex messages to the CLI" table.
use super::*;
use codex_router_test_support::node_taps::{
    EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode, RecordLayer, TapCase,
};
use session_event_model::{CapabilityReport, SessionItemKind, SessionSettings, SessionState};

#[path = "app_server_event_forwarding_node_tap_rig_test.rs"]
mod rig;
pub(super) use rig::*;

const ROW_SETTINGS: &str = "N3 catalog: SettingsChanged";
const ROW_STATE: &str = "N3 catalog: StateChanged";
const ROW_TURN_STARTED: &str = "N3 catalog: TurnStarted";
const ROW_TURN_ENDED: &str = "N3 catalog: TurnEnded";
const ROW_USAGE: &str = "N3 catalog: Item Usage";
const ROW_METADATA: &str = "N3 catalog: Item ModeChange, ConfigChange, SessionInfo, CommandCatalog";
const ROW_PLAN: &str = "N3 catalog: Item Plan";
const ROW_ACCEPTED: &str = "N3 catalog: InputAccepted";
const ROW_ZERO: &str = "N3 catalog: CapabilitiesChanged, ResyncRequired";

fn tap_settings() -> SessionSettings {
    SessionSettings {
        mode: None,
        model: Some("model-x".into()),
        effort: Some("high".into()),
        config: Vec::new(),
    }
}

#[test]
fn node_tap_settings_changed_with_working_directory() {
    let mut rig = Rig::new();
    rig.forwarding
        .register_working_directory(rig.session.clone(), PathBuf::from("/work/tap"));
    let row = expectation(
        &[frame(
            "thread/settings/updated",
            json!({"threadSettings":{
                "cwd":"/work/tap","disabledPluginIds":[],"approvalPolicy":"on-request",
                "approvalsReviewer":"user","sandboxPolicy":{"type":"dangerFullAccess"},
                "activePermissionProfile":null,"model":"model-x","modelProvider":"claude-local",
                "serviceTier":null,"effort":"high","summary":null,"personality":null,
                "collaborationMode":{"mode":"default","settings":{
                    "model":"model-x","reasoning_effort":"high","developer_instructions":null
                }}
            }}),
        )],
        base_state().field("working_directory", "registered"),
    );
    tap_event(
        &mut rig,
        "settings_changed_with_working_directory",
        ROW_SETTINGS,
        SessionEvent::SettingsChanged {
            settings: tap_settings(),
        },
        &row,
    );
}

#[test]
fn node_tap_settings_changed_without_working_directory() {
    let mut rig = Rig::new();
    let row = expectation(&[], base_state());
    tap_event(
        &mut rig,
        "settings_changed_without_working_directory",
        ROW_SETTINGS,
        SessionEvent::SettingsChanged {
            settings: tap_settings(),
        },
        &row,
    );
}

#[test]
fn node_tap_state_changed_open() {
    let mut rig = Rig::new();
    let row = expectation(
        &[frame(
            "thread/status/changed",
            json!({"status":{"type":"idle"}}),
        )],
        base_state(),
    );
    tap_event(
        &mut rig,
        "state_changed_idle",
        ROW_STATE,
        SessionEvent::StateChanged {
            state: SessionState::Idle,
        },
        &row,
    );
}

#[test]
fn node_tap_state_changed_closed() {
    let mut rig = Rig::new();
    let row = expectation(
        &[
            frame(
                "thread/status/changed",
                json!({"status":{"type":"notLoaded"}}),
            ),
            frame("thread/closed", json!({})),
        ],
        base_state(),
    );
    tap_event(
        &mut rig,
        "state_changed_closed",
        ROW_STATE,
        SessionEvent::StateChanged {
            state: SessionState::Closed,
        },
        &row,
    );
}

#[test]
fn node_tap_turn_started_accepts_receipt_and_records_turn() {
    let mut rig = Rig::new();
    let input_id = rig.register_user_input("client-1");
    let row = expectation(
        &[turn_frame(
            "turn/started",
            "turn-1",
            "inProgress",
            json!(null),
        )],
        base_state().field("active_turn", "turn-1").field(
            "receipts",
            "outstanding=[client-1@-] accepted_for_next_item=client-1",
        ),
    );
    tap_event(
        &mut rig,
        "turn_started",
        ROW_TURN_STARTED,
        SessionEvent::TurnStarted {
            turn_id: "turn-1".into(),
            input_id,
        },
        &row,
    );
}

fn turn_end_case(
    case_name: &str,
    outcome: TurnOutcome,
    expected_status: &str,
    expected_error: Value,
) {
    let mut rig = Rig::new();
    let input_id = rig.register_user_input("client-1");
    rig.setup(SessionEvent::TurnStarted {
        turn_id: "turn-1".into(),
        input_id,
    });
    rig.setup(started(agent_message("hi")));
    let row = expectation(
        &[turn_frame(
            "turn/completed",
            "turn-1",
            expected_status,
            expected_error,
        )],
        base_state().field(
            "receipts",
            "outstanding=[client-1@-] accepted_for_next_item=none",
        ),
    );
    tap_event(
        &mut rig,
        case_name,
        ROW_TURN_ENDED,
        SessionEvent::TurnEnded {
            turn_id: "turn-1".into(),
            outcome,
        },
        &row,
    );
}

#[test]
fn node_tap_turn_ended_cancelled_is_interrupted() {
    turn_end_case(
        "turn_ended_cancelled",
        TurnOutcome::Ended {
            stop_reason: StopReason::Cancelled,
            local_cause: None,
        },
        "interrupted",
        json!(null),
    );
}

#[test]
fn node_tap_turn_ended_ended_is_completed() {
    turn_end_case(
        "turn_ended_ended",
        TurnOutcome::Ended {
            stop_reason: StopReason::EndTurn,
            local_cause: None,
        },
        "completed",
        json!(null),
    );
}

#[test]
fn node_tap_turn_ended_lost_is_failed_with_reason() {
    turn_end_case(
        "turn_ended_lost",
        TurnOutcome::Lost {
            reason: TurnLostReason::ProviderTurnFailed,
        },
        "failed",
        json!({"message":"Provider Turn failed before completion","codexErrorInfo":null}),
    );
}

#[test]
fn node_tap_usage_in_turn_maps_window_and_used() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    let row = expectation(
        &[frame(
            "thread/tokenUsage/updated",
            json!({"turnId":"turn-1","tokenUsage":{
                "total":{"totalTokens":12,"inputTokens":0,"cachedInputTokens":0,"cacheWriteInputTokens":0,"outputTokens":0,"reasoningOutputTokens":0},
                "last":{"totalTokens":12,"inputTokens":0,"cachedInputTokens":0,"cacheWriteInputTokens":0,"outputTokens":0,"reasoningOutputTokens":0},
                "modelContextWindow":100
            }}),
        )],
        base_state().field("active_turn", "turn-1"),
    );
    tap_event(
        &mut rig,
        "usage_in_turn",
        ROW_USAGE,
        started(item(
            "u",
            SessionItemKind::Usage,
            Some(r#"{"used":12,"size":100}"#),
        )),
        &row,
    );
}

#[test]
fn node_tap_usage_out_of_turn_is_dropped() {
    let mut rig = Rig::new();
    let row = expectation(&[], base_state());
    tap_event(
        &mut rig,
        "usage_out_of_turn",
        ROW_USAGE,
        started(item(
            "u",
            SessionItemKind::Usage,
            Some(r#"{"used":12,"size":100}"#),
        )),
        &row,
    );
}

fn metadata_case(case_name: &str, metadata: SessionItem) {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    let row = expectation(&[], base_state().field("active_turn", "turn-1"));
    tap_event(&mut rig, case_name, ROW_METADATA, started(metadata), &row);
}

#[test]
fn node_tap_metadata_mode_change_is_dropped() {
    metadata_case(
        "mode_change",
        item("x", SessionItemKind::ModeChange, Some("default")),
    );
}

#[test]
fn node_tap_metadata_config_change_is_dropped() {
    metadata_case(
        "config_change",
        item("x", SessionItemKind::ConfigChange, Some("[]")),
    );
}

#[test]
fn node_tap_metadata_session_info_is_dropped() {
    metadata_case(
        "session_info",
        item(
            "x",
            SessionItemKind::SessionInfo,
            Some(r#"{"title":"Example"}"#),
        ),
    );
}

#[test]
fn node_tap_metadata_command_catalog_is_dropped() {
    metadata_case(
        "command_catalog",
        item(
            "x",
            SessionItemKind::Notice,
            Some(r#"{"availableCommands":[{"name":"compact","description":"Compact"}]}"#),
        ),
    );
}

fn plan_frame(explanation: &str) -> Value {
    frame(
        "turn/plan/updated",
        json!({"turnId":"turn-1","explanation":explanation,"plan":[]}),
    )
}

pub(super) fn plan(text: &str) -> SessionItem {
    item("p", SessionItemKind::Plan, Some(text))
}

#[test]
fn node_tap_plan_first_sighting_emits_plan_update() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    let row = expectation(
        &[plan_frame("Do X")],
        base_state()
            .field("active_turn", "turn-1")
            .field("stored_items", "p:plan"),
    );
    tap_event(
        &mut rig,
        "plan_started",
        ROW_PLAN,
        started(plan("Do X")),
        &row,
    );
}

#[test]
fn node_tap_plan_unchanged_update_is_suppressed() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    rig.setup(started(plan("Do X")));
    let row = expectation(
        &[],
        base_state()
            .field("active_turn", "turn-1")
            .field("stored_items", "p:plan"),
    );
    tap_event(
        &mut rig,
        "plan_update_unchanged",
        ROW_PLAN,
        updated(plan("Do X")),
        &row,
    );
}

#[test]
fn node_tap_plan_changed_update_emits_plan_update() {
    let mut rig = Rig::new();
    rig.start_turn("turn-1");
    rig.setup(started(plan("Do X")));
    let row = expectation(
        &[plan_frame("Do X then Y")],
        base_state()
            .field("active_turn", "turn-1")
            .field("stored_items", "p:plan"),
    );
    tap_event(
        &mut rig,
        "plan_update_changed",
        ROW_PLAN,
        updated(plan("Do X then Y")),
        &row,
    );
}

#[test]
fn node_tap_input_accepted_emits_nothing_and_receipts_accept() {
    let mut rig = Rig::new();
    let input_id = rig.register_user_input("client-1");
    let row = expectation(
        &[],
        base_state().field(
            "receipts",
            "outstanding=[client-1@-] accepted_for_next_item=client-1",
        ),
    );
    tap_event(
        &mut rig,
        "input_accepted",
        ROW_ACCEPTED,
        SessionEvent::InputAccepted {
            input_id,
            turn_id: "turn-1".into(),
        },
        &row,
    );
}

#[test]
fn node_tap_capabilities_changed_emits_nothing() {
    let mut rig = Rig::new();
    let row = expectation(&[], base_state());
    tap_event(
        &mut rig,
        "capabilities_changed",
        ROW_ZERO,
        SessionEvent::CapabilitiesChanged {
            capabilities: CapabilityReport::default(),
        },
        &row,
    );
}

#[test]
fn node_tap_resync_required_emits_nothing() {
    let mut rig = Rig::new();
    let row = expectation(&[], base_state());
    tap_event(
        &mut rig,
        "resync_required",
        ROW_ZERO,
        SessionEvent::ResyncRequired { replay_epoch: 2 },
        &row,
    );
}

fn user_item_completed() -> Value {
    item_completed_frame(
        "turn-1",
        json!({"type":"userMessage","id":"u","content":[{"type":"text","text":"hello"}],"clientId":"client-1"}),
    )
}

// Receipt state machine (node section N3): Outstanding -> Bound -> Retired.
const ROW_RECEIPTS: &str = "N3 node section: receipt state machine";

#[test]
fn node_tap_receipt_retired_after_frame_delivered() {
    let mut rig = Rig::new();
    let input_id = rig.register_user_input("client-1");
    rig.setup(SessionEvent::TurnStarted {
        turn_id: "turn-1".into(),
        input_id,
    });
    rig.setup(started(item(
        "u",
        SessionItemKind::UserMessage,
        Some("hello"),
    )));
    let mut case = TapCase::new(
        ProtocolNode::N3OutputMapper,
        "receipt_retired_on_delivered",
        ROW_RECEIPTS,
    );
    case.state_before(rig.snapshot());
    let event = SessionEvent::ItemCompleted {
        item_id: "u".into(),
    };
    case.consumed_with(
        RecordLayer::TypedInput,
        MessageFamily::SessionEvent,
        EdgeParty::Node(ProtocolNode::N5EventHub),
        &session_event_kind(&event),
        event_payload(&event),
    );
    let frames = rig.setup(event);
    record_frames(&mut case, &frames, CLOCK_FIELDS);
    // N1 calls `delivered` only after its send succeeded; the tap plays N1 here.
    rig.forwarding.user_receipts.delivered(&frames[0]);
    case.state_after(rig.snapshot());
    let row = expectation(
        &[user_item_completed()],
        in_turn().field("receipts", RECEIPTS_EMPTY),
    );
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_receipt_bound_item_is_not_retired_before_delivery() {
    let mut rig = Rig::new();
    let input_id = rig.register_user_input("client-1");
    rig.setup(SessionEvent::TurnStarted {
        turn_id: "turn-1".into(),
        input_id,
    });
    rig.setup(started(item(
        "u",
        SessionItemKind::UserMessage,
        Some("hello"),
    )));
    let row = expectation(
        &[user_item_completed()],
        in_turn().field(
            "receipts",
            "outstanding=[client-1@u] accepted_for_next_item=none",
        ),
    );
    tap_event(
        &mut rig,
        "receipt_bound_survives_until_delivered",
        ROW_RECEIPTS,
        SessionEvent::ItemCompleted {
            item_id: "u".into(),
        },
        &row,
    );
}

#[test]
fn node_tap_receipt_replay_reaccepts_input_without_rebinding() {
    let mut rig = Rig::new();
    let input_id = rig.register_user_input("client-1");
    rig.setup(SessionEvent::TurnStarted {
        turn_id: "turn-1".into(),
        input_id: input_id.clone(),
    });
    rig.setup(started(item(
        "u1",
        SessionItemKind::UserMessage,
        Some("hello"),
    )));
    let row = expectation(
        &[],
        in_turn().field("stored_items", "u1:userMessage").field(
            "receipts",
            "outstanding=[client-1@u1] accepted_for_next_item=client-1",
        ),
    );
    tap_event(
        &mut rig,
        "receipt_replay_reaccepts_bound_input",
        ROW_RECEIPTS,
        SessionEvent::InputAccepted {
            input_id,
            turn_id: "turn-1".into(),
        },
        &row,
    );
    // A second user item must not take the bound input's client id.
    let row = expectation(
        &[item_started_frame(
            "turn-1",
            json!({"type":"userMessage","id":"u2","content":[{"type":"text","text":"replayed"}]}),
        )],
        in_turn()
            .field("stored_items", "u1:userMessage,u2:userMessage")
            .field(
                "receipts",
                "outstanding=[client-1@u1] accepted_for_next_item=none",
            ),
    );
    tap_event(
        &mut rig,
        "receipt_replay_never_rebinds_to_another_item",
        ROW_RECEIPTS,
        started(item("u2", SessionItemKind::UserMessage, Some("replayed"))),
        &row,
    );
}

pub(super) fn in_turn() -> NodeStateSnapshot {
    base_state().field("active_turn", "turn-1")
}
