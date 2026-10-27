//! Failed tool and plan cursors must not complete inside a successor turn.

use super::*;
use session_event_model::{SessionItemKind, ToolCallStatus, TurnLostReason, TurnOutcome};

const FAILED_TOOL_PLAN_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-failed-turn-agent.ts"
);

#[expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
async fn collect_until(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
    observed: &mut Vec<SessionEvent>,
    matches_event: impl Fn(&SessionEvent) -> bool,
) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = events
                .recv()
                .await
                .expect("actual item emitter remains open");
            let reached = matches_event(&event);
            observed.push(event);
            if reached {
                break;
            }
        }
    })
    .await
    .expect("bounded actual event observation");
}

#[tokio::test]
async fn failed_tool_plan_cursors_are_not_successor_turn_items() {
    let root = tempfile::tempdir_in("/tmp").expect("private short-path fixture root");
    let control_path = root.path().join("control.sock");
    let receipt_path = root.path().join("wire-prompts.json");
    let control_listener = tokio::net::UnixListener::bind(&control_path).expect("control socket");
    let (event_sender, mut events) = tokio::sync::mpsc::unbounded_channel();
    let client = Arc::new(
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            AgentSessionClient::initialize(
                ExternalProviderLaunch {
                    executable: PathBuf::from("node"),
                    arguments: vec![
                        FAILED_TOOL_PLAN_FIXTURE.into(),
                        "tool-plan".into(),
                        control_path.to_string_lossy().into_owned(),
                        receipt_path.to_string_lossy().into_owned(),
                    ],
                    environment: Vec::new(),
                    persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
                },
                Arc::new(NoopInteractionPort),
                Arc::new(CapturedEventSink(event_sender)),
            ),
        )
        .await
        .expect("bounded fixture initialize")
        .expect("fixture initializes"),
    );
    let (mut control, _) =
        tokio::time::timeout(std::time::Duration::from_secs(2), control_listener.accept())
            .await
            .expect("bounded fixture control connection")
            .expect("control connection");
    let session_id = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.create_session(root.path().to_path_buf()),
    )
    .await
    .expect("bounded session creation")
    .expect("session opens");
    let first_input = InputId::generate();
    let second_input = InputId::generate();
    let first_client = Arc::clone(&client);
    let first_session = session_id.clone();
    let sent_first_input = first_input.clone();
    let first = tokio::spawn(async move {
        first_client
            .prompt_contents_with_approval_dispatch_for_input(
                first_session,
                sent_first_input,
                vec![text_prompt_content("FIRST_REQUEST")],
                101,
                None,
            )
            .await
    });
    let mut observed = Vec::new();
    collect_until(&mut events, &mut observed, |event| {
        matches!(event,
        SessionEvent::ItemStarted { item } if item.kind == SessionItemKind::Plan
            && item.text.as_deref() == Some("- Pending: FIRST_PLAN"))
    })
    .await;
    control
        .write_all(b"e")
        .await
        .expect("release original structured error");
    let first_result = tokio::time::timeout(std::time::Duration::from_secs(2), first)
        .await
        .expect("bounded first prompt settlement")
        .expect("first task joins");
    collect_until(&mut events, &mut observed, |event| {
        matches!(event, SessionEvent::TurnEnded { .. })
    })
    .await;
    let connection_survived_error = !client.retirement().is_cancelled();
    control
        .write_all(b"t")
        .await
        .expect("release old tool lookup update");
    collect_until(&mut events, &mut observed, |event| matches!(event,
        SessionEvent::ItemUpdated { item } if item.item_id == "failed-tool"
            && matches!(item.kind, SessionItemKind::ToolCall { status: ToolCallStatus::Completed, .. }))).await;
    let second_client = Arc::clone(&client);
    let sent_second_input = second_input.clone();
    let second = tokio::spawn(async move {
        second_client
            .prompt_contents_with_approval_dispatch_for_input(
                session_id,
                sent_second_input,
                vec![text_prompt_content("SECOND_REQUEST")],
                202,
                None,
            )
            .await
    });
    control
        .write_all(b"s")
        .await
        .expect("release explicit successor");
    let second_result = tokio::time::timeout(std::time::Duration::from_secs(2), second)
        .await
        .expect("bounded successor settlement")
        .expect("successor task joins");
    collect_until(&mut events, &mut observed, |event| {
        matches!(event, SessionEvent::TurnEnded { .. })
    })
    .await;
    let mut completed = [0u8; 1];
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        control.read_exact(&mut completed),
    )
    .await
    .expect("bounded fixture receipt")
    .expect("fixture completion receipt");
    control
        .write_all(b"q")
        .await
        .expect("release fixture idle phase");
    tokio::time::timeout(std::time::Duration::from_secs(5), client.shutdown())
        .await
        .expect("owned fixture actor and child shutdown");
    let wire: serde_json::Value =
        serde_json::from_slice(&std::fs::read(receipt_path).expect("wire prompt receipt"))
            .expect("wire receipt JSON");

    assert_eq!(completed, *b"d");
    assert_ne!(first_input, second_input);
    assert!(
        connection_survived_error,
        "structured error leaves transport alive"
    );
    assert!(matches!(
        first_result,
        Err(ExternalProviderRuntimeError::ProviderRejected { code: -32603, .. })
    ));
    assert_eq!(
        wire,
        serde_json::json!({"promptCount":2,
        "promptTexts":["FIRST_REQUEST","SECOND_REQUEST"]})
    );
    assert_eq!(
        second_result.expect("successor succeeds").output,
        "SECOND_ONLY"
    );
    assert_eq!(
        observed
            .iter()
            .filter(|event| matches!(event, SessionEvent::TurnEnded { .. }))
            .count(),
        2
    );
    let first_end = observed
        .iter()
        .position(|event| matches!(event, SessionEvent::TurnEnded { .. }))
        .expect("first turn terminal");
    assert!(matches!(
        observed[first_end],
        SessionEvent::TurnEnded {
            outcome: TurnOutcome::Lost {
                reason: TurnLostReason::ProviderTurnFailed
            },
            ..
        }
    ));
    let first_items = &observed[..first_end];
    assert!(first_items.iter().any(|event| matches!(event,
        SessionEvent::ItemStarted { item } if item.text.as_deref() == Some("FIRST_PARTIAL"))));
    assert!(first_items.iter().any(|event| matches!(event,
        SessionEvent::ItemStarted { item } if item.item_id == "failed-tool"
            && item.text.as_deref() == Some("FIRST_TOOL")
            && item.kind == (SessionItemKind::ToolCall { tool_kind:"execute".into(), status:ToolCallStatus::Pending }))));
    let first_plan = first_items
        .iter()
        .find_map(|event| match event {
            SessionEvent::ItemStarted { item } if item.kind == SessionItemKind::Plan => Some(item),
            _ => None,
        })
        .expect("actual first plan observed before original error");
    let second_start = observed
        .iter()
        .position(|event| {
            matches!(event,
        SessionEvent::TurnStarted { input_id, .. } if input_id == &second_input)
        })
        .expect("distinct successor admitted");
    assert!(observed[first_end..second_start].iter().any(|event| matches!(event,
        SessionEvent::ItemUpdated { item } if item.item_id == "failed-tool"
            && item.text.as_deref() == Some("FIRST_TOOL")
            && item.kind == (SessionItemKind::ToolCall { tool_kind:"execute".into(), status:ToolCallStatus::Completed }))),
        "idle tool update must retain actual kind/title lookup without a new tool item");
    let successor_items = &observed[second_start..];
    assert!(!successor_items.iter().any(|event| matches!(event,
        SessionEvent::ItemCompleted { item_id } if item_id == "failed-tool" || item_id == &first_plan.item_id)),
        "failed tool/plan completion crossed successor admission; old plan {}; actual successor events: {successor_items:?}", first_plan.item_id);
    assert!(
        successor_items.iter().any(|event| matches!(event,
        SessionEvent::ItemStarted { item } if item.kind == SessionItemKind::Plan
            && item.item_id != first_plan.item_id
            && item.text.as_deref() == Some("- Pending: SECOND_PLAN"))),
        "successor plan must start a distinct SECOND_PLAN item: {successor_items:?}"
    );
}
