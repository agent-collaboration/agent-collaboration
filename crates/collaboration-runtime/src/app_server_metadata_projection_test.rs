use super::*;
use session_event_model::{InputId, SessionItemKind};

fn metadata_items() -> Vec<SessionItem> {
    [
        (SessionItemKind::Usage, r#"{"used":12,"size":100,"cost":{"amount":0.25,"currency":"USD"}}"#),
        (SessionItemKind::Notice, r#"{"availableCommands":[{"name":"compact","description":"Compact context","input":{"hint":"instructions"}}],"_meta":{"vendor":"retained"}}"#),
        (SessionItemKind::Notice, r#"{"availableCommands":[]}"#),
        (SessionItemKind::ModeChange, "default"),
        (SessionItemKind::ConfigChange, "[]"),
        (SessionItemKind::SessionInfo, r#"{"title":"Example session"}"#),
    ].into_iter().enumerate().map(|(index, (kind, text))| SessionItem {
        item_id: format!("metadata-{index}"), kind, text: Some(text.into()),
    }).collect()
}

fn display_items() -> Vec<SessionItem> {
    [
        (SessionItemKind::AgentMessage, "Ordinary assistant prose"),
        (SessionItemKind::AgentThought, "Useful reasoning"),
        (SessionItemKind::Plan, "Inspect then build"),
        (
            SessionItemKind::ToolCall {
                tool_kind: "read".into(),
                status: session_event_model::ToolCallStatus::Completed,
            },
            "Read completed",
        ),
        (SessionItemKind::Notice, "Provider reconnect complete"),
        (
            SessionItemKind::Notice,
            r#"{"message":"A meaningful structured notice"}"#,
        ),
        (
            SessionItemKind::Notice,
            "The provider mentioned availableCommands in a diagnostic",
        ),
        (
            SessionItemKind::Unknown {
                source_kind: "future_notice".into(),
            },
            "Future useful notice",
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (kind, text))| SessionItem {
        item_id: format!("display-{index}"),
        kind,
        text: Some(text.into()),
    })
    .collect()
}

fn target() -> SessionRef {
    serde_json::from_value(
        json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001",
        "endpointId":"claude-local"},"sessionId":"metadata-session"}),
    )
    .expect("session")
}

#[test]
fn live_metadata_lifecycle_has_no_transcript_frames_and_preserves_real_content() {
    let session = target();
    let actor: Identity =
        serde_json::from_value(json!({"kind":"human","humanId":"owner"})).expect("actor");
    let mut forwarding = AppServerEventForwarding::new(actor, None);
    forwarding.project(
        &session,
        &HubEvent {
            sequence: 1,
            event: SessionEvent::TurnStarted {
                turn_id: "turn-one".into(),
                input_id: InputId::generate(),
            },
        },
    );
    for item in metadata_items() {
        for event in [
            SessionEvent::ItemStarted { item: item.clone() },
            SessionEvent::ItemUpdated { item: item.clone() },
            SessionEvent::ItemCompleted {
                item_id: item.item_id.clone(),
            },
        ] {
            assert!(
                forwarding
                    .project(&session, &HubEvent { sequence: 2, event })
                    .is_empty(),
                "metadata {} became a transcript frame",
                item.item_id
            );
        }
    }
    for item in display_items() {
        let frames = forwarding.project(
            &session,
            &HubEvent {
                sequence: 3,
                event: SessionEvent::ItemStarted { item: item.clone() },
            },
        );
        assert!(
            !frames.is_empty(),
            "genuine content {} disappeared",
            item.item_id
        );
        let completed = forwarding.project(
            &session,
            &HubEvent {
                sequence: 4,
                event: SessionEvent::ItemCompleted {
                    item_id: item.item_id.clone(),
                },
            },
        );
        if item.kind != SessionItemKind::Plan {
            assert_eq!(completed.len(), 1);
        }
    }
}

#[test]
fn completed_and_active_history_exclude_metadata_and_preserve_transcript_content() {
    for active in [false, true] {
        let mut items = metadata_items();
        let expected = display_items();
        items.extend(expected.clone());
        let mut snapshot = vec![HubEvent {
            sequence: 1,
            event: SessionEvent::TurnStarted {
                turn_id: "turn-one".into(),
                input_id: InputId::generate(),
            },
        }];
        for (index, item) in items.iter().enumerate() {
            snapshot.push(HubEvent {
                sequence: (index + 2) as u64,
                event: SessionEvent::ItemStarted { item: item.clone() },
            });
        }
        if !active {
            snapshot.push(HubEvent {
                sequence: 100,
                event: SessionEvent::TurnEnded {
                    turn_id: "turn-one".into(),
                    outcome: TurnOutcome::Ended {
                        stop_reason: StopReason::EndTurn,
                        local_cause: None,
                    },
                },
            });
        }
        let turns = historical_turns(&target(), &snapshot);
        assert_eq!(turns.len(), 1);
        let rendered = turns
            .first()
            .and_then(|turn| turn.get("items"))
            .and_then(Value::as_array)
            .expect("transcript items");
        assert_eq!(rendered.len(), expected.len());
        assert_eq!(
            rendered
                .iter()
                .map(|item| item.get("id").and_then(Value::as_str).expect("id"))
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|item| item.item_id.as_str())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn metadata_only_history_has_no_turn_until_the_first_genuine_exchange() {
    let session = target();
    let mut snapshot = metadata_items()
        .into_iter()
        .enumerate()
        .map(|(index, item)| HubEvent {
            sequence: index as u64,
            event: SessionEvent::ItemStarted { item },
        })
        .collect::<Vec<_>>();
    assert_eq!(historical_turns(&session, &snapshot), Vec::<Value>::new());

    let prompt = SessionItem {
        item_id: "first-user-message".into(),
        kind: SessionItemKind::UserMessage,
        text: Some("What is 2 + 2?".into()),
    };
    let reply = SessionItem {
        item_id: "first-assistant-reply".into(),
        kind: SessionItemKind::AgentMessage,
        text: Some("4".into()),
    };
    snapshot.push(HubEvent {
        sequence: 20,
        event: SessionEvent::TurnStarted {
            turn_id: "first-live-turn".into(),
            input_id: InputId::generate(),
        },
    });
    for (index, item) in [prompt, reply].into_iter().enumerate() {
        snapshot.push(HubEvent {
            sequence: 21 + index as u64,
            event: SessionEvent::ItemStarted { item },
        });
    }
    snapshot.push(HubEvent {
        sequence: 23,
        event: SessionEvent::TurnEnded {
            turn_id: "first-live-turn".into(),
            outcome: TurnOutcome::Ended {
                stop_reason: StopReason::EndTurn,
                local_cause: None,
            },
        },
    });

    let turns = historical_turns(&session, &snapshot);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0]["status"], "completed");
    assert_eq!(
        turns[0]["items"],
        json!([
            {"type":"userMessage","id":"first-user-message",
             "content":[{"type":"text","text":"What is 2 + 2?"}]},
            {"type":"agentMessage","id":"first-assistant-reply","text":"4"}
        ])
    );
    assert_eq!(historical_turns(&session, &snapshot), turns);

    snapshot.push(HubEvent {
        sequence: 24,
        event: SessionEvent::ItemStarted {
            item: SessionItem {
                item_id: "second-user-message".into(),
                kind: SessionItemKind::UserMessage,
                text: Some("Thank you".into()),
            },
        },
    });
    let extended = historical_turns(&session, &snapshot);
    assert_eq!(extended.len(), 2);
    assert_eq!(extended[0], turns[0]);
}

#[test]
fn empty_active_history_turn_remains_visible_before_and_after_metadata() {
    let session = target();
    let mut snapshot = vec![HubEvent {
        sequence: 1,
        event: SessionEvent::TurnStarted {
            turn_id: "empty-active-turn".into(),
            input_id: InputId::generate(),
        },
    }];
    let expected = json!([{
        "id":"empty-active-turn","items":[],"itemsView":"full",
        "status":"inProgress","error":null,
        "startedAt":null,"completedAt":null,"durationMs":null
    }]);
    assert_eq!(json!(historical_turns(&session, &snapshot)), expected);
    for (index, item) in metadata_items().into_iter().enumerate() {
        snapshot.push(HubEvent {
            sequence: 2 + index as u64,
            event: SessionEvent::ItemStarted { item },
        });
    }
    assert_eq!(json!(historical_turns(&session, &snapshot)), expected);
}
