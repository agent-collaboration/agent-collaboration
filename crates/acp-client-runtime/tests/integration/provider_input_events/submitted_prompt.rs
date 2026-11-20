//! Submitted tasks enter history independently of provider user-message groups.

use super::*;
use acp_client_runtime::{ExternalProviderPromptOutcome, ExternalProviderRuntimeError};
use serde_json::json;
use session_event_model::{PromptContent, SessionItemKind};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

const PROVIDER_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-submitted-prompt-agent.ts"
);

#[derive(Clone, Copy)]
enum RejectedUserEvent {
    Started,
    Completed,
}

#[derive(Default)]
struct SubmittedEventSink {
    events: Mutex<Vec<SessionEvent>>,
    reject: Option<RejectedUserEvent>,
    rejected: AtomicBool,
}

impl SessionEventSink for SubmittedEventSink {
    fn publish_skill_view(
        &self,
        _session_id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed> {
        Ok(())
    }

    fn begin_history_replay(&self, _session_id: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    fn publish(&self, _session_id: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        if matches!(
            (&self.reject, &event),
            (Some(RejectedUserEvent::Started), SessionEvent::ItemStarted { item })
                if item.kind == SessionItemKind::UserMessage
        ) || matches!(
            (&self.reject, &event),
            (
                Some(RejectedUserEvent::Completed),
                SessionEvent::ItemCompleted { .. }
            )
        ) {
            self.rejected.store(true, Ordering::Relaxed);
            return Err(EventSinkClosed);
        }
        self.events.lock().map_err(|_| EventSinkClosed)?.push(event);
        Ok(())
    }
}

struct PromptFixture {
    root: tempfile::TempDir,
    client: AgentSessionClient<NoopInteractionPort>,
    session_id: String,
    sink: Arc<SubmittedEventSink>,
}

#[expect(
    clippy::expect_used,
    reason = "All failed fixture setup is an assertion failure."
)]
impl PromptFixture {
    async fn open(mode: &str, reject: Option<RejectedUserEvent>) -> Self {
        let root = tempfile::tempdir().expect("fixture directory");
        let receipt = root.path().join("prompt-receipt.jsonl");
        std::fs::write(&receipt, "").expect("empty wire receipt");
        let sink = Arc::new(SubmittedEventSink {
            reject,
            ..Default::default()
        });
        let client = AgentSessionClient::initialize(
            ExternalProviderLaunch {
                executable: PathBuf::from("node"),
                arguments: vec![
                    PROVIDER_FIXTURE.into(),
                    mode.into(),
                    receipt.display().to_string(),
                ],
                environment: Vec::new(),
                persistence_target: ProviderPersistenceTarget::Unspecified,
            },
            Arc::new(NoopInteractionPort),
            sink.clone(),
            Arc::new(crate::test_behavior::TestProviderBehavior),
        )
        .await
        .expect("provider initializes");
        let session_id = client
            .create_session(root.path().to_owned())
            .await
            .expect("session opens");
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
        blocks: Vec<PromptContent>,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        self.client
            .submit_with_approval_dispatch_for_input(
                self.session_id.clone(),
                InputId::generate(),
                acp_client_runtime::ProviderTurnInput::Prompt(
                    session_event_model::ProviderSubmission::plain(blocks),
                ),
                (),
                None,
            )
            .await
    }

    fn wire_prompts(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.root.path().join("prompt-receipt.jsonl"))
            .expect("wire receipt")
            .lines()
            .map(|line| serde_json::from_str(line).expect("wire JSON"))
            .collect()
    }

    async fn shutdown(&self) {
        self.client.shutdown().await;
    }
}

#[expect(clippy::expect_used, reason = "The scripted text block must be valid.")]
fn text_block(text: &str) -> PromptContent {
    serde_json::from_value(json!({"type":"text","text":text})).expect("text block")
}

#[tokio::test]
async fn submitted_text_has_fresh_completed_history_items_without_merging_provider_echoes() {
    const TASK: &str = "/chosen  λ\n$HOME";
    for mode in ["none", "echo", "different", "reject"] {
        // Arrange: only the vendor peer is substituted; the ACP actor and sink are real.
        let fixture = PromptFixture::open(mode, None).await;
        // Act: identical admitted tasks must remain separate submissions.
        for _ in 0..2 {
            let result = fixture.prompt(vec![text_block(TASK)]).await;
            assert_eq!(result.is_err(), mode == "reject");
        }
        fixture.shutdown().await;
        assert_eq!(
            fixture.wire_prompts(),
            vec![json!([{"type":"text","text":TASK}]); 2]
        );
        // Assert: the submitted item precedes native groups and keeps exact bytes.
        let events = fixture.sink.events.lock().expect("events lock");
        let turns: Vec<_> = events
            .split(|event| matches!(event, SessionEvent::TurnStarted { .. }))
            .skip(1)
            .collect();
        assert_eq!(turns.len(), 2);
        let mut submitted_ids = Vec::new();
        for (index, turn) in turns.iter().enumerate() {
            let Some(SessionEvent::ItemStarted { item }) = turn.first() else {
                panic!("submitted UserMessage must immediately follow TurnStarted: {turn:?}");
            };
            assert_eq!(item.kind, SessionItemKind::UserMessage);
            assert_eq!(item.text.as_deref(), Some(TASK));
            assert_eq!(
                uuid::Uuid::parse_str(&item.item_id)
                    .expect("submitted UUID")
                    .get_version_num(),
                7
            );
            assert!(
                matches!(&turn[1], SessionEvent::ItemCompleted { item_id } if item_id == &item.item_id)
            );
            submitted_ids.push(item.item_id.clone());
            let users: Vec<_> = turn
                .iter()
                .filter_map(|event| match event {
                    SessionEvent::ItemStarted { item }
                        if item.kind == SessionItemKind::UserMessage =>
                    {
                        Some(item)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                users.len(),
                if matches!(mode, "echo" | "different") {
                    2
                } else {
                    1
                }
            );
            if let Some(native) = users.get(1) {
                assert_eq!(native.item_id, format!("native-{}", index + 1));
                assert_ne!(native.item_id, item.item_id);
                let prefix = if mode == "echo" {
                    TASK
                } else {
                    "Native different task"
                };
                assert_eq!(native.text.as_deref(), Some(prefix));
                assert!(turn.iter().any(|event| matches!(event, SessionEvent::ItemUpdated { item: update }
                    if update.item_id == native.item_id && update.text.as_deref() == Some(format!("{prefix} + native tail").as_str()))));
                assert!(turn.iter().any(|event| matches!(event, SessionEvent::ItemCompleted { item_id } if item_id == &native.item_id)));
            }
        }
        assert_ne!(submitted_ids[0], submitted_ids[1]);
    }

    let fixture = PromptFixture::open("none", None).await;
    let resource: PromptContent = serde_json::from_value(
        json!({"type":"resourceLink","uri":"file:///reference","name":"reference"}),
    )
    .expect("resource block");
    for blocks in [
        vec![],
        vec![text_block("one"), text_block("two")],
        vec![text_block("task"), resource.clone()],
        vec![resource],
    ] {
        fixture
            .prompt(blocks)
            .await
            .expect("non-single-Text prompt");
    }
    let wire_before_rejection = fixture.wire_prompts();
    let audio =
        serde_json::from_value(json!({"type":"audio","mimeType":"audio/wav","data":"aGVsbG8="}))
            .expect("audio block");
    assert!(matches!(
        fixture.prompt(vec![audio]).await,
        Err(ExternalProviderRuntimeError::UnsupportedContent { .. })
    ));
    fixture.shutdown().await;
    let events_before_retired = fixture.sink.events.lock().expect("events lock").len();
    assert!(
        fixture
            .prompt(vec![text_block("retired transport")])
            .await
            .is_err()
    );
    assert_eq!(
        fixture.sink.events.lock().expect("events lock").len(),
        events_before_retired
    );
    assert_eq!(fixture.wire_prompts(), wire_before_rejection);
    let events = fixture.sink.events.lock().expect("events lock");
    let receipts: Vec<_> = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            SessionEvent::ItemStarted { item } if item.kind == SessionItemKind::UserMessage => {
                Some((index, item))
            }
            _ => None,
        })
        .collect();
    assert_eq!(receipts.len(), 4);
    for ((index, item), expected) in receipts.iter().zip(["", "onetwo", "task", ""]) {
        assert_eq!(item.text.as_deref(), Some(expected));
        assert_eq!(
            events[index + 1],
            SessionEvent::ItemCompleted {
                item_id: item.item_id.clone()
            }
        );
    }
}

#[tokio::test]
async fn submitted_user_item_refusal_preserves_the_actual_prompt_result() {
    for reject in [RejectedUserEvent::Started, RejectedUserEvent::Completed] {
        let fixture = PromptFixture::open("none", Some(reject)).await;
        let result = fixture.prompt(vec![text_block("sent task")]).await;
        fixture.shutdown().await;
        assert_eq!(
            result.expect("provider prompt still completes").stop_reason,
            session_event_model::StopReason::EndTurn
        );
        assert!(fixture.sink.rejected.load(Ordering::Relaxed));
        assert_eq!(
            fixture.wire_prompts(),
            [json!([{"type":"text","text":"sent task"}])]
        );
        let events = fixture.sink.events.lock().expect("events lock");
        assert!(matches!(
            events.first(),
            Some(SessionEvent::TurnStarted { .. })
        ));
        let users = events
            .iter()
            .filter_map(|event| match event {
                SessionEvent::ItemStarted { item } if item.kind == SessionItemKind::UserMessage => {
                    Some(&item.item_id)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            users.len(),
            usize::from(matches!(reject, RejectedUserEvent::Completed))
        );
        assert!(!events.iter().any(|event| matches!(event, SessionEvent::ItemCompleted { item_id } if users.contains(&item_id))));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, SessionEvent::TurnEnded { .. }))
        );
    }
}

/// A media-only prompt is still a committed user input; compact remains separate.
#[tokio::test]
async fn submitted_media_prompts_always_have_one_completed_user_item() {
    // Arrange: real prompt admission and actor, with a media-capable scripted peer.
    let fixture = PromptFixture::open("none", None).await;
    let image: PromptContent =
        serde_json::from_value(json!({"type":"image","mimeType":"image/png","data":"AA=="}))
            .expect("image");
    // Act: both a mixed input and one with no text must be represented.
    fixture
        .prompt(vec![text_block("left"), image.clone(), text_block("right")])
        .await
        .expect("mixed prompt");
    fixture
        .prompt(vec![image])
        .await
        .expect("image-only prompt");
    fixture.shutdown().await;
    // Assert: literal text projection, one completion with the same item ID.
    let events = fixture.sink.events.lock().expect("events");
    let receipts: Vec<_> = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            SessionEvent::ItemStarted { item } if item.kind == SessionItemKind::UserMessage => {
                Some((index, item))
            }
            _ => None,
        })
        .collect();
    assert_eq!(receipts.len(), 2);
    for ((index, item), expected) in receipts.iter().zip(["leftright", ""]) {
        assert_eq!(item.text.as_deref(), Some(expected));
        assert_eq!(
            events[index + 1],
            SessionEvent::ItemCompleted {
                item_id: item.item_id.clone()
            }
        );
    }
}
