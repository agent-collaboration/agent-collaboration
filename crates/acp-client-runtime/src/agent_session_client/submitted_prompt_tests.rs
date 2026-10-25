//! Submitted tasks and native user groups remain independent at the real actor.
#![allow(clippy::expect_used)]

use super::turn_admission_tests::NoopInteractionPort;
use super::*;
use crate::{EventSinkClosed, HistoryReplayFuture, SessionEventSink};
use agent_client_protocol::schema::v1::ContentBlock;
use session_event_model::{SessionEvent, SessionItemKind};
use std::{path::PathBuf, sync::Mutex, time::Duration};

// The shell process replaces only the provider. The SDK, actor and event sink
// are real; no model execution or installed-provider behavior is claimed.
const PROMPT_FIXTURE: &str = r#"
prompt_count=0
while IFS= read -r request; do
  printf '%s\n' "$request" >> "$PROMPT_TRACE"
  request_id=$(printf '%s\n' "$request" | sed -n 's/.*"id":\([^,}]*\).*/\1/p')
  case "$request" in
    *'"method":"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentCapabilities":{},"agentInfo":{"name":"submitted-task-fixture","version":"1"}}}\n' "$request_id" ;;
    *'"method":"session/new"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"submitted-task-session"}}\n' "$request_id" ;;
    *'"method":"session/prompt"'*)
      prompt_count=$((prompt_count + 1))
      if test "$USER_MODE" != none; then
        if test "$USER_MODE" = echo; then
          native_text=$(printf '%s\n' "$request" | sed -En 's/.*"text":("(\\.|[^"\\])*").*/\1/p')
        else native_text='"Native different task"'; fi
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"submitted-task-session","update":{"sessionUpdate":"user_message_chunk","messageId":"native-%s","content":{"type":"text","text":%s}}}}\n' "$prompt_count" "$native_text"
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"submitted-task-session","update":{"sessionUpdate":"user_message_chunk","messageId":"native-%s","content":{"type":"text","text":" + native tail"}}}}\n' "$prompt_count"
        if test "$USER_MODE" = different; then
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"submitted-task-session","update":{"sessionUpdate":"user_message_chunk","messageId":"native-other-%s","content":{"type":"text","text":"Another native group"}}}}\n' "$prompt_count"
        fi
      fi
      if test "$FAIL_PROMPT" = yes; then
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32603,"message":"fixture prompt rejection"}}\n' "$request_id"
      else
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"submitted-task-session","update":{"sessionUpdate":"agent_message_chunk","messageId":"reply-%s","content":{"type":"text","text":"Actual fixture reply"}}}}\n' "$prompt_count"
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$request_id"
      fi ;;
  esac
done
"#;

#[derive(Default)]
struct SubmittedEventSink {
    events: Mutex<Vec<(String, SessionEvent)>>,
    reject: Option<&'static str>,
}

impl SessionEventSink for SubmittedEventSink {
    fn begin_history_replay(&self, _session_id: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    fn publish(&self, session_id: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        let stage = match &event {
            SessionEvent::TurnStarted { .. } => "turn",
            SessionEvent::ItemStarted { .. } => "item",
            SessionEvent::ItemCompleted { .. } => "completion",
            _ => "other",
        };
        if self.reject == Some(stage) {
            return Err(EventSinkClosed);
        }
        self.events
            .lock()
            .expect("events lock")
            .push((session_id.into(), event));
        Ok(())
    }
}

struct PromptFixture {
    root: tempfile::TempDir,
    client: AgentSessionClient<NoopInteractionPort>,
    session_id: String,
    sink: Arc<SubmittedEventSink>,
}

impl PromptFixture {
    async fn new(mode: &str, fail: bool, reject: Option<&'static str>) -> Self {
        let root = tempfile::tempdir_in("/tmp").expect("owned prompt fixture");
        let trace = root.path().join("wire.jsonl");
        std::fs::write(&trace, "").expect("wire receipt");
        let sink = Arc::new(SubmittedEventSink {
            reject,
            ..Default::default()
        });
        let client = tokio::time::timeout(
            Duration::from_secs(5),
            AgentSessionClient::initialize(
                ExternalProviderLaunch {
                    executable: PathBuf::from("/bin/sh"),
                    arguments: vec!["-c".into(), PROMPT_FIXTURE.into()],
                    environment: vec![
                        ("PROMPT_TRACE".into(), trace.display().to_string()),
                        ("USER_MODE".into(), mode.into()),
                        ("FAIL_PROMPT".into(), if fail { "yes" } else { "no" }.into()),
                    ],
                    persistence_target: crate::ProviderPersistenceTarget::Unspecified,
                },
                Arc::new(NoopInteractionPort),
                sink.clone(),
            ),
        )
        .await
        .expect("bounded initialization")
        .expect("initialize");
        let session_id = tokio::time::timeout(
            Duration::from_secs(5),
            client.create_session(root.path().into()),
        )
        .await
        .expect("bounded session creation")
        .expect("session");
        sink.events.lock().expect("events lock").clear();
        Self {
            root,
            client,
            session_id,
            sink,
        }
    }

    async fn prompt(
        &self,
        input_id: InputId,
        blocks: Vec<ContentBlock>,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        tokio::time::timeout(
            Duration::from_secs(5),
            self.client
                .prompt_content(self.session_id.clone(), input_id, None, blocks, None),
        )
        .await
        .expect("bounded prompt")
    }

    fn wire_prompts(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.root.path().join("wire.jsonl"))
            .expect("wire receipt")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("wire JSON"))
            .filter(|request| request["method"] == "session/prompt")
            .collect()
    }

    async fn shutdown(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.client.shutdown())
            .await
            .expect("owned fixture shutdown");
    }
}

fn text_block(text: &str) -> ContentBlock {
    serde_json::from_value(serde_json::json!({"type":"text","text":text})).expect("text block")
}

#[tokio::test]
async fn submitted_tasks_keep_exact_bytes_and_fresh_ids_without_coalescing_native_groups() {
    const TASK: &str = "/chosen  λ\n$HOME";
    for mode in ["none", "echo", "different"] {
        let fixture = PromptFixture::new(mode, false, None).await;
        let inputs = [InputId::generate(), InputId::generate()];
        for input_id in &inputs {
            assert_eq!(
                fixture
                    .prompt(input_id.clone(), vec![text_block(TASK)])
                    .await
                    .expect("prompt")
                    .output,
                "Actual fixture reply"
            );
        }
        fixture.shutdown().await;
        let wire = fixture.wire_prompts();
        assert_eq!(wire.len(), 2, "separately accepted identical tasks");
        for request in wire {
            assert_eq!(
                request["params"]["prompt"],
                serde_json::json!([{"type":"text","text":TASK}])
            );
        }
        let events = fixture.sink.events.lock().expect("events lock");
        assert!(
            events
                .iter()
                .all(|(session, _)| session == &fixture.session_id)
        );
        let starts = events
            .iter()
            .enumerate()
            .filter_map(|(index, (_, event))| match event {
                SessionEvent::TurnStarted { input_id, .. } => Some((index, input_id.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            starts
                .iter()
                .map(|(_, input)| input.clone())
                .collect::<Vec<_>>(),
            inputs
        );
        let mut submitted_ids = Vec::new();
        for (admission, (start, _)) in starts.iter().enumerate() {
            let end = starts
                .get(admission + 1)
                .map_or(events.len(), |(index, _)| *index);
            let turn = &events[*start + 1..end];
            let SessionEvent::ItemStarted { item: submitted } = &turn[0].1 else {
                panic!("submitted item must follow TurnStarted: {turn:?}");
            };
            assert_eq!(submitted.kind, SessionItemKind::UserMessage);
            assert_eq!(submitted.text.as_deref(), Some(TASK));
            assert!(
                matches!(&turn[1].1, SessionEvent::ItemCompleted { item_id } if item_id == &submitted.item_id)
            );
            submitted_ids.push(submitted.item_id.clone());
            let users = turn
                .iter()
                .filter_map(|(_, event)| match event {
                    SessionEvent::ItemStarted { item }
                        if item.kind == SessionItemKind::UserMessage =>
                    {
                        Some(item)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                users.len(),
                match mode {
                    "none" => 1,
                    "echo" => 2,
                    _ => 3,
                }
            );
            if mode != "none" {
                let native_id = format!("native-{}", admission + 1);
                assert_eq!(users[1].item_id, native_id);
                assert_ne!(submitted.item_id, native_id);
                let native_prefix = if mode == "echo" {
                    TASK
                } else {
                    "Native different task"
                };
                assert_eq!(users[1].text.as_deref(), Some(native_prefix));
                assert!(turn.iter().any(|(_, event)| matches!(event, SessionEvent::ItemUpdated { item }
                    if item.item_id == native_id && item.text.as_deref() == Some(format!("{native_prefix} + native tail").as_str()))));
                assert!(turn.iter().any(|(_, event)| matches!(event, SessionEvent::ItemCompleted { item_id } if item_id == &native_id)));
                if mode == "different" {
                    let other_id = format!("native-other-{}", admission + 1);
                    assert_eq!(users[2].item_id, other_id);
                    assert_eq!(users[2].text.as_deref(), Some("Another native group"));
                    assert!(turn.iter().any(|(_, event)| matches!(event, SessionEvent::ItemCompleted { item_id } if item_id == &other_id)));
                }
            }
        }
        assert_ne!(
            submitted_ids[0], submitted_ids[1],
            "accepted repetitions are distinct submissions"
        );
    }
}

#[tokio::test]
async fn non_single_text_and_pre_send_rejections_add_no_submitted_user_item() {
    let fixture = PromptFixture::new("none", false, None).await;
    let resource: ContentBlock = serde_json::from_value(
        serde_json::json!({"type":"resource_link","name":"reference","uri":"file:///reference"}),
    )
    .expect("resource link");
    for blocks in [
        vec![],
        vec![text_block("one"), text_block("two")],
        vec![resource.clone()],
        vec![text_block("task"), resource],
    ] {
        fixture
            .prompt(InputId::generate(), blocks)
            .await
            .expect("unchanged non-single-Text submission");
    }
    let before_rejections = fixture.wire_prompts();
    assert_eq!(before_rejections.len(), 4);
    fixture.client.settings_unresolved.write().await.insert(
        fixture.session_id.clone(),
        std::collections::HashMap::from([(
            crate::ProviderSettingKind::Mode,
            crate::provider_session_settings::UnresolvedSettingCause::DefiniteFailure,
        )]),
    );
    assert!(matches!(
        fixture
            .prompt(InputId::generate(), vec![text_block("rejected settings")])
            .await,
        Err(ExternalProviderRuntimeError::SettingsUnresolved)
    ));
    let audio = serde_json::from_value(
        serde_json::json!({"type":"audio","mimeType":"audio/wav","data":"aGVsbG8="}),
    )
    .expect("audio block");
    assert!(matches!(
        fixture.prompt(InputId::generate(), vec![audio]).await,
        Err(ExternalProviderRuntimeError::UnsupportedContent { .. })
    ));
    fixture.shutdown().await;
    let before_retired = fixture.sink.events.lock().expect("events lock").len();
    assert!(
        fixture
            .prompt(InputId::generate(), vec![text_block("retired transport")])
            .await
            .is_err()
    );
    assert_eq!(
        fixture.sink.events.lock().expect("events lock").len(),
        before_retired,
        "retired submission adds no events"
    );
    assert_eq!(
        fixture.wire_prompts(),
        before_rejections,
        "rejections never reach provider"
    );
    assert!(!fixture.sink.events.lock().expect("events lock").iter().any(|(_, event)| matches!(event, SessionEvent::ItemStarted { item } if item.kind == SessionItemKind::UserMessage)));
}

#[tokio::test]
async fn provider_error_after_local_send_keeps_submitted_task() {
    let fixture = PromptFixture::new("none", true, None).await;
    assert!(
        fixture
            .prompt(InputId::generate(), vec![text_block("sent task")])
            .await
            .is_err()
    );
    fixture.shutdown().await;
    assert_eq!(fixture.wire_prompts().len(), 1);
    let events = fixture.sink.events.lock().expect("events lock");
    let users = events
        .iter()
        .filter_map(|(_, event)| match event {
            SessionEvent::ItemStarted { item } if item.kind == SessionItemKind::UserMessage => {
                Some(item)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].text.as_deref(), Some("sent task"));
    assert!(events.iter().any(|(_, event)| matches!(event, SessionEvent::ItemCompleted { item_id } if item_id == &users[0].item_id)));
}

#[tokio::test]
async fn submitted_item_sink_failure_preserves_post_send_sink_closed_error() {
    for stage in ["turn", "item", "completion"] {
        let fixture = PromptFixture::new("none", false, Some(stage)).await;
        assert!(matches!(
            fixture
                .prompt(InputId::generate(), vec![text_block("sent task")])
                .await,
            Err(ExternalProviderRuntimeError::SinkClosed)
        ));
        assert!(fixture.client.sink_closed.is_cancelled());
        fixture.shutdown().await;
        let events = fixture.sink.events.lock().expect("events lock");
        let users = events.iter().filter(|(_, event)| matches!(event, SessionEvent::ItemStarted { item } if item.kind == SessionItemKind::UserMessage)).count();
        assert_eq!(users, usize::from(stage == "completion"));
        assert!(
            !events
                .iter()
                .any(|(_, event)| matches!(event, SessionEvent::ItemCompleted { .. }))
        );
    }
}
