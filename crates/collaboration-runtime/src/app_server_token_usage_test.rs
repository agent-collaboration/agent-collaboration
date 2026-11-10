use crate::app_server_event_forwarding::AppServerEventForwarding;
use crate::router_session_app_server::thread_alias;
use collaboration_contracts::HubEvent;
use message_board::{Identity, SessionRef};
use serde_json::{Value, json};
use session_event_model::{
    InputId, SessionEvent, SessionItem, SessionItemKind, StopReason, TurnOutcome,
};

fn usage_fixture() -> (SessionRef, AppServerEventForwarding) {
    let session = serde_json::from_value(json!({
        "endpoint":{"serviceId":uuid::Uuid::now_v7(),"endpointId":"claude-local"},
        "sessionId":uuid::Uuid::now_v7()
    }))
    .expect("session");
    let actor: Identity =
        serde_json::from_value(json!({"kind":"human","humanId":"owner"})).expect("actor");
    (session, AppServerEventForwarding::new(actor, None))
}

fn start_turn(forwarding: &mut AppServerEventForwarding, session: &SessionRef) {
    forwarding.project(
        session,
        &HubEvent {
            sequence: 1,
            event: SessionEvent::TurnStarted {
                turn_id: "usage-turn".into(),
                input_id: InputId::generate(),
            },
        },
    );
}

fn usage_item(text: Option<&str>) -> SessionItem {
    SessionItem {
        item_id: "usage-item".into(),
        kind: SessionItemKind::Usage,
        text: text.map(str::to_owned),
    }
}

fn project_usage(
    forwarding: &mut AppServerEventForwarding,
    session: &SessionRef,
    text: Option<&str>,
) -> Vec<Value> {
    forwarding.project(
        session,
        &HubEvent {
            sequence: 2,
            event: SessionEvent::ItemStarted {
                item: usage_item(text),
            },
        },
    )
}

// Codex 360319d8 app-server-protocol/src/protocol/v2/thread.rs:1952-2040;
// tui/token_usage.rs consumes last.totalTokens for context remaining.
fn expected_usage(session: &SessionRef) -> Value {
    let breakdown = json!({"totalTokens":506000,"inputTokens":0,"cachedInputTokens":0,
        "cacheWriteInputTokens":0,"outputTokens":0,"reasoningOutputTokens":0});
    json!({"method":"thread/tokenUsage/updated","params":{
        "threadId":thread_alias(session),"turnId":"usage-turn",
        "tokenUsage":{"total":breakdown,"last":breakdown,"modelContextWindow":1000000}
    }})
}

#[test]
fn live_usage_projects_one_notification_with_context_occupancy() {
    // Arrange
    let (session, mut forwarding) = usage_fixture();
    start_turn(&mut forwarding, &session);
    // Act
    let frames = project_usage(
        &mut forwarding,
        &session,
        Some(r#"{"used":506000,"size":1000000,"cost":{"amount":0.25,"currency":"USD"}}"#),
    );
    // Assert: ACP used is the latest occupancy, not an accumulated billable count.
    assert_eq!(frames, vec![expected_usage(&session)]);
}

#[test]
fn malformed_usage_emits_nothing_after_a_valid_update() {
    // Arrange
    let (session, mut forwarding) = usage_fixture();
    start_turn(&mut forwarding, &session);
    // Act / Assert: positive control rejects an implementation that drops all usage.
    assert_eq!(
        project_usage(
            &mut forwarding,
            &session,
            Some(r#"{"used":506000,"size":1000000}"#)
        ),
        vec![expected_usage(&session)]
    );
    for text in [
        None,
        Some("not json"),
        Some("{}"),
        Some(r#"{"used":12}"#),
        Some(r#"{"size":1000000}"#),
        Some(r#"{"used":null,"size":1000000}"#),
        Some(r#"{"used":-1,"size":1000000}"#),
        Some(r#"{"used":1,"size":0}"#),
        Some(r#"{"used":1.5,"size":1000000}"#),
        Some(r#"{"used":"1","size":1000000}"#),
        Some(r#"{"used":1,"size":9223372036854775808}"#),
    ] {
        assert!(
            project_usage(&mut forwarding, &session, text).is_empty(),
            "invalid usage {text:?}"
        );
    }
}

#[test]
fn usage_lifecycle_emits_only_usage_notifications_and_no_item_frames() {
    // Arrange
    let (session, mut forwarding) = usage_fixture();
    start_turn(&mut forwarding, &session);
    let item = usage_item(Some(r#"{"used":506000,"size":1000000}"#));
    // Act
    let mut frames = Vec::new();
    for event in [
        SessionEvent::ItemStarted { item: item.clone() },
        SessionEvent::ItemUpdated { item: item.clone() },
        SessionEvent::ItemCompleted {
            item_id: item.item_id,
        },
    ] {
        frames.extend(forwarding.project(&session, &HubEvent { sequence: 2, event }));
    }
    // Assert: one notification per supplied occupancy update, none on completion.
    assert_eq!(
        frames,
        vec![expected_usage(&session), expected_usage(&session)]
    );
    assert!(!frames.iter().any(|frame| {
        frame["method"]
            .as_str()
            .is_some_and(|method| method.starts_with("item/"))
    }));
}

#[test]
fn usage_without_an_active_turn_never_invents_a_turn() {
    // Arrange
    let (session, mut forwarding) = usage_fixture();
    let text = Some(r#"{"used":506000,"size":1000000}"#);
    // Act / Assert
    assert!(project_usage(&mut forwarding, &session, text).is_empty());
    start_turn(&mut forwarding, &session);
    forwarding.project(
        &session,
        &HubEvent {
            sequence: 3,
            event: SessionEvent::TurnEnded {
                turn_id: "usage-turn".into(),
                outcome: TurnOutcome::Ended {
                    stop_reason: StopReason::EndTurn,
                    local_cause: None,
                },
            },
        },
    );
    assert!(project_usage(&mut forwarding, &session, text).is_empty());
}
