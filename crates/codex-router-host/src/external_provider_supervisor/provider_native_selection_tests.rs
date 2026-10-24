//! Real Host preparation with stored target, live runtime and native file reader.
//! ACP and CLI fixture processes stand in for the installed provider.
use super::*;
use collaboration_service::{
    CommandContent, CreateSessionCommand, SelectedItem, SelectedPromptCommand,
    SelectedPromptPreparation, SelectionFailure, SessionCommandPort, SessionSettingsCommand,
};
use session_event_model::{InputId, SessionEvent, SessionItem, SessionItemKind};

#[tokio::test]
async fn native_capabilities_selected_host_prepares_delivers_once_and_preserves_history() {
    let mut fixture = CatalogueFixture::new(ProviderKind::ClaudeCode).await;
    let actor: message_board::Identity =
        serde_json::from_value(json!({"kind":"human","humanId":"catalogue-fixture-owner"}))
            .expect("actor");
    let target = tokio::time::timeout(
        Duration::from_secs(5),
        fixture.commands.create(CreateSessionCommand {
            endpoint: fixture.board_endpoint.clone(),
            working_directory: fixture.workspace.clone(),
            settings: SessionSettingsCommand::default(),
            actor: actor.clone(),
        }),
    )
    .await
    .expect("bounded create")
    .expect("real create");
    let path = fixture.workspace.join(".claude/skills/chosen/SKILL.md");
    write(&path, "Native chosen body $ARGUMENTS");
    let command = SelectedPromptCommand {
        target: target.clone(),
        input_id: InputId::generate(),
        actor,
        content: vec![CommandContent::text("$chosen  λ\n$HOME".into()).expect("text")],
        selections: vec![SelectedItem::Skill {
            submitted_name: CapabilityName::new("chosen".into()).expect("name"),
            submitted_path: CapabilityPath::new(
                fs::canonicalize(&path).expect("native file identity"),
            )
            .expect("path"),
        }],
    };
    // Native advertisements are retained in the real hub; these notices are
    // controlled fixture inputs, not an installed-native observation.
    for text in [
        r#"{"availableCommands":[{"name":"old","description":"Old"}]}"#,
        r#"{"availableCommands":[{"name":"chosen","description":"Chosen"}]}"#,
    ] {
        fixture
            .hub
            .publish(
                target.clone(),
                SessionEvent::ItemStarted {
                    item: SessionItem {
                        item_id: uuid::Uuid::new_v4().to_string(),
                        kind: SessionItemKind::Notice,
                        text: Some(text.into()),
                    },
                },
            )
            .await
            .expect("retained metadata");
    }
    let prepared = fixture
        .commands
        .prepare_selected_prompt(command.clone())
        .await
        .expect("real preparation");
    assert_eq!(
        prepared,
        SelectedPromptPreparation::Ready {
            content: vec![
                CommandContent::text("/chosen  λ\n$HOME".into()).expect("canonical text")
            ]
        }
    );
    let after_valid = fs::read_to_string(&fixture.trace).expect("ACP trace");
    assert_eq!(
        after_valid.lines().count(),
        1,
        "preparation must retain only session/new, with no prompt/steer"
    );
    assert!(after_valid.contains("session/new"));
    let mut foreign = command.clone();
    foreign.selections = vec![SelectedItem::Skill {
        submitted_name: CapabilityName::new("chosen".into()).expect("name"),
        submitted_path: CapabilityPath::new(fixture.workspace.join("foreign/SKILL.md"))
            .expect("foreign identity"),
    }];
    assert_eq!(
        fixture
            .commands
            .prepare_selected_prompt(foreign)
            .await
            .expect("reject"),
        SelectedPromptPreparation::Rejected {
            reason: SelectionFailure::WrongContext
        }
    );
    write(
        &fixture.workspace.join(".claude/settings.local.json"),
        r#"{"skillOverrides":{"chosen":"off"}}"#,
    );
    assert_eq!(
        fixture
            .commands
            .prepare_selected_prompt(command.clone())
            .await
            .expect("disabled"),
        SelectedPromptPreparation::Rejected {
            reason: SelectionFailure::Disabled
        }
    );
    assert_eq!(
        fs::read_to_string(&fixture.trace).expect("trace"),
        after_valid
    );
    // Exercise the full facade -> Host -> existing prompt delivery -> ACP path.
    write(&fixture.workspace.join(".claude/settings.local.json"), "{}");
    let listed = fixture.rpc_params("thread/list", json!({})).await;
    let thread_id = listed["result"]["data"][0]["id"].clone();
    assert!(
        thread_id.is_string(),
        "real stored session has a thread alias"
    );
    let submitted = fixture.rpc_params("turn/start", json!({"threadId":thread_id,"input":[
        {"type":"text","text":"$chosen  λ\n$HOME"},
        {"type":"skill","name":"chosen","path":fs::canonicalize(&path).expect("actual path")}
    ]})).await;
    assert_eq!(
        submitted["result"]["turn"]["status"], "inProgress",
        "{submitted}"
    );
    // TurnStarted precedes dispatch. Completion is the causal receipt for
    // inspecting transport input and historical state, without a timed sleep.
    let turn_id = submitted["result"]["turn"]["id"].as_str().expect("turn ID");
    let mut completion =
        collaboration_service::SessionEventHub::attach(fixture.hub.as_ref(), target.clone())
            .await
            .expect("completion subscription");
    let ended = |event: &SessionEvent| matches!(event, SessionEvent::TurnEnded { turn_id: observed, .. } if observed == turn_id);
    if !completion.snapshot.iter().any(|event| ended(&event.event)) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if ended(
                    &completion
                        .receiver
                        .recv()
                        .await
                        .expect("completion event")
                        .event,
                ) {
                    break;
                }
            }
        })
        .await
        .expect("actual prompt completes");
    }
    let after_prompt = fs::read_to_string(&fixture.trace).expect("ACP input trace");
    let requests: Vec<Value> = after_prompt
        .lines()
        .map(|line| serde_json::from_str(line).expect("actual ACP request"))
        .collect();
    assert_eq!(
        requests.len(),
        2,
        "one create and one prompt, no steer or queue fallback"
    );
    let prompt = requests
        .iter()
        .find(|request| request["method"] == "session/prompt")
        .expect("actual prompt");
    assert_eq!(prompt["params"]["prompt"][0]["text"], "/chosen  λ\n$HOME");
    let native_reads = fs::read_to_string(&fixture.native_export_trace).expect("native read trace");
    for method in ["thread/read", "thread/resume"] {
        let history = fixture
            .rpc_params(method, json!({"threadId":thread_id,"includeTurns":true}))
            .await;
        assert!(history.get("result").is_some(), "{history}");
        let turns = history["result"]["thread"]["turns"]
            .as_array()
            .expect("historical turns");
        let user_tasks = turns
            .iter()
            .filter_map(|turn| turn.get("items").and_then(Value::as_array))
            .flatten()
            .filter(|item| item["type"] == "userMessage")
            .collect::<Vec<_>>();
        assert_eq!(
            user_tasks.len(),
            1,
            "exactly one selected user task, without duplicates or an original dollar task: {history}"
        );
        assert_eq!(
            user_tasks.first().expect("canonical task").get("content"),
            Some(&json!([{"type":"text","text":"/chosen  λ\n$HOME"}]))
        );
        assert_eq!(
            fs::read_to_string(&fixture.trace).expect("no replay dispatch"),
            after_prompt
        );
        assert_eq!(
            fs::read_to_string(&fixture.native_export_trace).expect("no replay discovery"),
            native_reads
        );
    }
    write(&fixture.hold_prompt, "hold the next ordinary fixture turn");
    let mut busy_receipt =
        collaboration_service::SessionEventHub::attach(fixture.hub.as_ref(), target.clone())
            .await
            .expect("busy receipt subscription");
    fixture
        .commands
        .prompt(collaboration_service::PromptSessionCommand {
            target: target.clone(),
            input_id: InputId::generate(),
            actor: command.actor.clone(),
            content: vec![CommandContent::text("Ordinary busy control".into()).expect("text")],
        })
        .await
        .expect("real ordinary turn starts and remains active");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if matches!(busy_receipt.receiver.recv().await.expect("busy receipt").event,
                SessionEvent::ItemStarted { ref item } | SessionEvent::ItemUpdated { ref item }
                if item.kind == SessionItemKind::AgentMessage && item.text.as_deref() == Some("Fixture held prompt received")) { break; }
        }
    }).await.expect("fixture confirms held request before the no-submit baseline");
    let before_busy = fs::read_to_string(&fixture.trace).expect("busy baseline");
    let queue_target = collaboration_service::SessionTargetCommand {
        target: target.clone(),
        actor: command.actor.clone(),
    };
    let before_queue = fixture
        .commands
        .queue_list(queue_target.clone())
        .await
        .expect("actual queue before rejection");
    assert!(before_queue.is_empty());
    assert!(matches!(
        fixture
            .commands
            .prepare_selected_prompt(command.clone())
            .await,
        Err(collaboration_service::CommandFailure::Busy)
    ));
    assert_eq!(
        fixture
            .commands
            .queue_list(queue_target.clone())
            .await
            .expect("actual queue after direct rejection"),
        before_queue
    );
    let rejected = fixture.rpc_params("turn/start", json!({"threadId":thread_id,"input":[
        {"type":"text","text":"$chosen rejected while busy"},
        {"type":"skill","name":"chosen","path":fs::canonicalize(&path).expect("native path")}
    ]})).await;
    assert!(rejected.get("error").is_some(), "{rejected}");
    assert_eq!(
        fixture
            .commands
            .queue_list(queue_target)
            .await
            .expect("actual queue after both rejections"),
        before_queue,
        "a Router enqueue must fail even when it emits no ACP request"
    );
    assert_eq!(
        fs::read_to_string(&fixture.trace).expect("busy has no dispatch"),
        before_busy
    );
    assert_eq!(
        fs::read_to_string(&fixture.native_export_trace).expect("busy has no discovery"),
        native_reads
    );
    fixture
        .supervisor
        .runtime_for(&fixture.endpoint)
        .expect("runtime")
        .retirement()
        .cancel();
    assert_eq!(
        fixture
            .commands
            .prepare_selected_prompt(command)
            .await
            .expect("retired selection"),
        SelectedPromptPreparation::Rejected {
            reason: SelectionFailure::WrongContext
        }
    );
    fixture.stop().await;
}
