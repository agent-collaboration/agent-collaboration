use super::*;
use session_event_model::SessionItemKind;

// Pinned Codex 360319d8 chatwidget/protocol.rs:122-147, 569-592:
// item starts establish identity; only deltas feed live message/reasoning text.
fn assert_live_delta_text(kind: SessionItemKind, initial: &str, updates: &[&str], expected: &str) {
    let session: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001",
        "endpointId":"claude-local"},"sessionId":"stream-delta-session"
    }))
    .expect("session");
    let actor: Identity =
        serde_json::from_value(json!({"kind":"human","humanId":"owner"})).expect("actor");
    let mut projector = AppServerEventForwarding::new(actor, None);
    projector.project(
        &session,
        &HubEvent {
            sequence: 1,
            event: SessionEvent::TurnStarted {
                turn_id: "stream-turn".into(),
                input_id: session_event_model::InputId::generate(),
            },
        },
    );
    let item = |text: &str| SessionItem {
        item_id: "stream-item".into(),
        kind: kind.clone(),
        text: Some(text.into()),
    };
    let mut frames = projector.project(
        &session,
        &HubEvent {
            sequence: 2,
            event: SessionEvent::ItemStarted {
                item: item(initial),
            },
        },
    );
    assert_eq!(frames[0]["method"], "item/started");
    for (index, text) in updates.iter().enumerate() {
        frames.extend(projector.project(
            &session,
            &HubEvent {
                sequence: 3 + index as u64,
                event: SessionEvent::ItemUpdated { item: item(text) },
            },
        ));
    }
    let method = match kind {
        SessionItemKind::AgentMessage => "item/agentMessage/delta",
        SessionItemKind::AgentThought => "item/reasoning/summaryTextDelta",
        _ => panic!("fixture must use a live text kind"),
    };
    let mut rendered = String::new();
    for frame in frames.iter().filter(|frame| frame["method"] == method) {
        assert_eq!(frame["params"]["threadId"], thread_alias(&session));
        assert_eq!(frame["params"]["turnId"], "stream-turn");
        assert_eq!(frame["params"]["itemId"], "stream-item");
        if kind == SessionItemKind::AgentThought {
            assert_eq!(frame["params"]["summaryIndex"], 0);
        }
        let delta = frame["params"]["delta"].as_str().expect("text delta");
        assert!(!delta.is_empty(), "empty live delta");
        rendered.push_str(delta);
    }
    assert_eq!(rendered, expected, "delta consumer lost or duplicated text");
    let completed = projector.project(
        &session,
        &HubEvent {
            sequence: 100,
            event: SessionEvent::ItemCompleted {
                item_id: "stream-item".into(),
            },
        },
    );
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0]["method"], "item/completed");
    let expected_item = match kind {
        SessionItemKind::AgentMessage => {
            json!({"type":"agentMessage","id":"stream-item","text":expected})
        }
        SessionItemKind::AgentThought => {
            json!({"type":"reasoning","id":"stream-item",
                "summary":if expected.is_empty() {vec![]} else {vec![expected]},"content":[]})
        }
        _ => panic!("fixture must use a live text kind"),
    };
    assert_eq!(completed[0]["params"]["item"], expected_item);
}

#[test]
fn initial_message_delta_reaches_the_live_consumer() {
    assert_live_delta_text(
        SessionItemKind::AgentMessage,
        "100",
        &["100000000 + 23456789 = 123456789"],
        "100000000 + 23456789 = 123456789",
    );
}

#[test]
fn initial_thought_delta_reaches_the_live_consumer() {
    assert_live_delta_text(
        SessionItemKind::AgentThought,
        "First",
        &["First more"],
        "First more",
    );
}

#[test]
fn initial_text_delta_handles_empty_single_and_unchanged_chunks() {
    for kind in [SessionItemKind::AgentMessage, SessionItemKind::AgentThought] {
        assert_live_delta_text(kind.clone(), "", &[], "");
        assert_live_delta_text(kind.clone(), "", &["Hello", "Hello"], "Hello");
        assert_live_delta_text(kind.clone(), "Hello", &[], "Hello");
        assert_live_delta_text(
            kind,
            "Hello",
            &["Hello", "Hello world", "Hello world"],
            "Hello world",
        );
    }
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn lost_turn_completion_has_a_typed_error_message() -> Result<(), Box<dyn std::error::Error>> {
    let session: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"claude-local"},
        "sessionId":"provider-1"
    }))?;
    let lost = render_turn_event(
        &session,
        &HubEvent {
            sequence: 2,
            event: SessionEvent::TurnEnded {
                turn_id: "turn-1".into(),
                outcome: TurnOutcome::Lost {
                    reason: session_event_model::TurnLostReason::ProviderRetired,
                },
            },
        },
    )
    .ok_or("turn completion")?;
    assert_eq!(lost["params"]["turn"]["status"], "failed");
    assert!(
        lost["params"]["turn"]["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("retired"))
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn item_completion_without_an_active_turn_is_not_sent() -> Result<(), Box<dyn std::error::Error>> {
    let session: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"claude-local"},
        "sessionId":"provider-1"
    }))?;
    let actor: Identity = serde_json::from_value(json!({"kind":"human","humanId":"owner"}))?;
    let mut projector = AppServerEventForwarding::new(actor, None);
    projector.project(
        &session,
        &HubEvent {
            sequence: 1,
            event: SessionEvent::ItemStarted {
                item: SessionItem {
                    item_id: "orphan".into(),
                    kind: SessionItemKind::AgentMessage,
                    text: Some("orphan".into()),
                },
            },
        },
    );
    let completed = projector.project(
        &session,
        &HubEvent {
            sequence: 2,
            event: SessionEvent::ItemCompleted {
                item_id: "orphan".into(),
            },
        },
    );
    assert!(completed.is_empty());
    Ok(())
}

/// Oracle: pinned Codex app-server-protocol v2/item.rs:1449-1457.
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn reasoning_update_streams_summary_text_delta() -> Result<(), Box<dyn std::error::Error>> {
    let session: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"claude-local"},
        "sessionId":"provider-1"
    }))?;
    let actor: Identity = serde_json::from_value(json!({"kind":"human","humanId":"owner"}))?;
    let mut projector = AppServerEventForwarding::new(actor, None);
    projector.project(
        &session,
        &HubEvent {
            sequence: 1,
            event: SessionEvent::TurnStarted {
                turn_id: "turn-1".into(),
                input_id: session_event_model::InputId::generate(),
            },
        },
    );
    let thought = |text: &str| SessionItem {
        item_id: "thought-1".into(),
        kind: SessionItemKind::AgentThought,
        text: Some(text.into()),
    };
    projector.project(
        &session,
        &HubEvent {
            sequence: 2,
            event: SessionEvent::ItemStarted {
                item: thought("First"),
            },
        },
    );
    let updated = projector.project(
        &session,
        &HubEvent {
            sequence: 3,
            event: SessionEvent::ItemUpdated {
                item: thought("First more"),
            },
        },
    );
    assert_eq!(updated[0]["method"], "item/reasoning/summaryTextDelta");
    assert_eq!(updated[0]["params"]["delta"], " more");
    assert_eq!(updated[0]["params"]["summaryIndex"], 0);
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn snapshot_includes_the_running_turn_and_its_latest_items()
-> Result<(), Box<dyn std::error::Error>> {
    let session: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"claude-local"},
        "sessionId":"provider-1"
    }))?;
    let snapshot = vec![
        HubEvent {
            sequence: 1,
            event: SessionEvent::TurnStarted {
                turn_id: "running-turn".into(),
                input_id: session_event_model::InputId::generate(),
            },
        },
        HubEvent {
            sequence: 2,
            event: SessionEvent::ItemStarted {
                item: SessionItem {
                    item_id: "reply".into(),
                    kind: SessionItemKind::AgentMessage,
                    text: Some("first".into()),
                },
            },
        },
        HubEvent {
            sequence: 3,
            event: SessionEvent::ItemUpdated {
                item: SessionItem {
                    item_id: "reply".into(),
                    kind: SessionItemKind::AgentMessage,
                    text: Some("first second".into()),
                },
            },
        },
    ];
    let turns = historical_turns(&session, &snapshot);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0]["id"], "running-turn");
    assert_eq!(turns[0]["status"], "inProgress");
    assert_eq!(turns[0]["items"][0]["text"], "first second");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn plan_item_projects_only_one_plan_surface() -> Result<(), Box<dyn std::error::Error>> {
    let session: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"claude-local"},
        "sessionId":"provider-1"
    }))?;
    let actor: Identity = serde_json::from_value(json!({"kind":"human","humanId":"owner"}))?;
    let mut projector = AppServerEventForwarding::new(actor, None);
    projector.project(
        &session,
        &HubEvent {
            sequence: 1,
            event: SessionEvent::TurnStarted {
                turn_id: "turn-1".into(),
                input_id: session_event_model::InputId::generate(),
            },
        },
    );
    let plan = |text: &str| SessionItem {
        item_id: "plan-1".into(),
        kind: SessionItemKind::Plan,
        text: Some(text.into()),
    };
    let started = projector.project(
        &session,
        &HubEvent {
            sequence: 2,
            event: SessionEvent::ItemStarted {
                item: plan("Inspect"),
            },
        },
    );
    assert_eq!(started.len(), 1);
    assert_eq!(started[0]["method"], "turn/plan/updated");
    let unchanged = projector.project(
        &session,
        &HubEvent {
            sequence: 3,
            event: SessionEvent::ItemUpdated {
                item: plan("Inspect"),
            },
        },
    );
    assert!(unchanged.is_empty());
    let updated = projector.project(
        &session,
        &HubEvent {
            sequence: 4,
            event: SessionEvent::ItemUpdated {
                item: plan("Inspect then edit"),
            },
        },
    );
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0]["method"], "turn/plan/updated");
    let completed = projector.project(
        &session,
        &HubEvent {
            sequence: 5,
            event: SessionEvent::ItemCompleted {
                item_id: "plan-1".into(),
            },
        },
    );
    assert!(completed.is_empty());
    Ok(())
}

/// Oracle: Codex 0.157.1 app-server-protocol v2/item.rs:1327-1336,
/// 1405-1414,1427-1433 and protocol/common.rs:1936-1946.
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn item_lifecycle_uses_typed_v2_notifications() -> Result<(), Box<dyn std::error::Error>> {
    let session: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"claude-local"},
        "sessionId":"provider-1"
    }))?;
    let actor: Identity = serde_json::from_value(json!({"kind":"human","humanId":"owner"}))?;
    let mut projector = AppServerEventForwarding::new(actor, None);
    let turn = projector.project(
        &session,
        &HubEvent {
            sequence: 1,
            event: SessionEvent::TurnStarted {
                turn_id: "turn-1".into(),
                input_id: session_event_model::InputId::generate(),
            },
        },
    );
    assert_eq!(turn[0]["method"], "turn/started");
    let item = |text: &str| SessionItem {
        item_id: "reply-1".into(),
        kind: SessionItemKind::AgentMessage,
        text: Some(text.into()),
    };
    let started = projector.project(
        &session,
        &HubEvent {
            sequence: 2,
            event: SessionEvent::ItemStarted {
                item: item("First"),
            },
        },
    );
    assert_eq!(started[0]["method"], "item/started");
    assert!(started[0]["params"]["startedAtMs"].is_i64());
    let updated = projector.project(
        &session,
        &HubEvent {
            sequence: 3,
            event: SessionEvent::ItemUpdated {
                item: item("First streamed"),
            },
        },
    );
    assert_eq!(updated[0]["method"], "item/agentMessage/delta");
    assert_eq!(updated[0]["params"]["delta"], " streamed");
    let completed = projector.project(
        &session,
        &HubEvent {
            sequence: 4,
            event: SessionEvent::ItemCompleted {
                item_id: "reply-1".into(),
            },
        },
    );
    assert_eq!(completed[0]["method"], "item/completed");
    assert_eq!(completed[0]["params"]["turnId"], "turn-1");
    assert_eq!(completed[0]["params"]["item"]["text"], "First streamed");
    assert!(completed[0]["params"]["completedAtMs"].is_i64());
    let ended = projector.project(
        &session,
        &HubEvent {
            sequence: 5,
            event: SessionEvent::TurnEnded {
                turn_id: "turn-1".into(),
                outcome: TurnOutcome::Ended {
                    stop_reason: StopReason::EndTurn,
                    local_cause: None,
                },
            },
        },
    );
    assert_eq!(ended[0]["method"], "turn/completed");
    Ok(())
}
