use acp_client_runtime::*;
use session_event_model::*;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
const BOUND: Duration = Duration::from_secs(3);

struct NoopInteractions;
impl InteractionPort for NoopInteractions {
    type Context = u64;
    type OperationId = u64;
    fn operation_id(context: &u64) -> u64 {
        *context
    }
    fn binding_retirement(_: &u64) -> CancellationToken {
        CancellationToken::new()
    }
    fn request_approval(
        &self,
        _: u64,
        _: ApprovalRequest,
        _: CancellationToken,
        _: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        Box::pin(async { ApprovalPortOutcome::Cancelled })
    }
    fn request_question(
        &self,
        _: u64,
        _: QuestionRequest,
        _: CancellationToken,
        _: CancellationToken,
    ) -> InteractionFuture<'_, QuestionResponse> {
        Box::pin(async { QuestionResponse::Cancelled })
    }
    fn record_refusal(&self, _: u64, _: RefusedApprovalOffer) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
    fn cancel_all(&self, _: u64, _: &'static str) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
    fn cancel_retired(&self) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
}

struct CapturedSink {
    events: Mutex<Vec<SessionEvent>>,
    views: tokio::sync::mpsc::UnboundedSender<SkillView>,
    published: tokio::sync::Notify,
}
impl SessionEventSink for CapturedSink {
    fn begin_history_replay(&self, _: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }
    fn publish_skill_view(&self, _: &str, view: SkillView) -> Result<(), EventSinkClosed> {
        self.views.send(view).map_err(|_| EventSinkClosed)
    }
    fn publish(&self, _: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        self.events.lock().map_err(|_| EventSinkClosed)?.push(event);
        self.published.notify_one();
        Ok(())
    }
}

enum Preparation {
    Preserve,
    Literal(&'static str),
}
struct ScriptedBehavior {
    skill: &'static str,
    preparation: Preparation,
}
impl ProviderBehavior for ScriptedBehavior {
    fn observe(&self, update: ProviderUpdate) -> Result<ProviderFacts, ProviderError> {
        let mut facts = ProviderFacts::default();
        match update {
            ProviderUpdate::SessionAdmitted | ProviderUpdate::SessionClosed => {
                facts.skill_view = Some(SkillView::Unknown)
            }
            ProviderUpdate::Commands { commands, .. } => {
                facts.skill_view = Some(SkillView::Available {
                    skills: if commands.is_empty() {
                        vec![]
                    } else {
                        vec![ProviderSkill {
                            invocation_name: SkillInvocationName::new(self.skill)
                                .map_err(|_| ProviderError::MalformedMetadata)?,
                            description: "fixture skill".into(),
                            scope: ProviderSkillScope::System,
                        }]
                    },
                    errors: vec![],
                })
            }
            ProviderUpdate::ToolStatus {
                tool_id,
                status,
                metadata,
            } => {
                if let Some(hint) = metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("hint"))
                    .and_then(serde_json::Value::as_str)
                {
                    let trigger = match hint {
                        "manual" => CompactionTrigger::Manual,
                        "automatic" => CompactionTrigger::Automatic,
                        _ => return Err(ProviderError::MalformedMetadata),
                    };
                    facts.compaction = Some(ProviderCompactionHint {
                        tool_id,
                        status,
                        trigger,
                    });
                }
            }
        }
        Ok(facts)
    }
    fn prepare(&self, submission: ProviderSubmission) -> Result<Vec<PromptContent>, ProviderError> {
        match self.preparation {
            Preparation::Preserve => Ok(submission.content),
            Preparation::Literal(output) => Ok(vec![
                PromptContent::text(output.into()).map_err(|_| ProviderError::InvalidSelection)?,
            ]),
        }
    }
    fn compact(&self) -> Result<PreparedCompact, ProviderError> {
        Ok(PreparedCompact::Prompt(vec![
            PromptContent::text("/compact".into()).map_err(|_| ProviderError::Unsupported)?,
        ]))
    }
}

struct Fixture {
    root: tempfile::TempDir,
    client: Arc<AgentSessionClient<NoopInteractions>>,
    session: String,
    sink: Arc<CapturedSink>,
    views: tokio::sync::mpsc::UnboundedReceiver<SkillView>,
}
impl Fixture {
    async fn open(
        behavior: ScriptedBehavior,
        compact_case: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let (sender, views) = tokio::sync::mpsc::unbounded_channel();
        let sink = Arc::new(CapturedSink {
            events: Mutex::new(vec![]),
            views: sender,
            published: tokio::sync::Notify::new(),
        });
        let client = Arc::new(
            tokio::time::timeout(
                BOUND,
                AgentSessionClient::initialize(
                    ExternalProviderLaunch {
                        executable: PathBuf::from("node"),
                        arguments: vec![
                            concat!(
                                env!("CARGO_MANIFEST_DIR"),
                                "/tests/support/behavior-agent.ts"
                            )
                            .into(),
                            root.path().join("wire.jsonl").display().to_string(),
                            compact_case.into(),
                        ],
                        environment: vec![],
                        persistence_target: ProviderPersistenceTarget::Unspecified,
                    },
                    Arc::new(NoopInteractions),
                    sink.clone(),
                    Arc::new(behavior),
                ),
            )
            .await??,
        );
        let session =
            tokio::time::timeout(BOUND, client.create_session(root.path().into())).await??;
        let mut fixture = Self {
            root,
            client,
            session,
            sink,
            views,
        };
        fixture.available().await?;
        fixture
            .sink
            .events
            .lock()
            .map_err(|_| "captured events poisoned")?
            .clear();
        Ok(fixture)
    }
    async fn available(&mut self) -> Result<SkillView, Box<dyn std::error::Error>> {
        tokio::time::timeout(BOUND, async {
            loop {
                let view = self.views.recv().await.ok_or("view channel closed")?;
                if matches!(view, SkillView::Available { .. }) {
                    break Ok::<_, Box<dyn std::error::Error>>(view);
                }
            }
        })
        .await?
    }
    async fn submit(
        &self,
        input: ProviderTurnInput,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        tokio::time::timeout(
            BOUND,
            self.client.submit_with_approval_dispatch_for_input(
                self.session.clone(),
                InputId::generate(),
                input,
                1,
                None,
            ),
        )
        .await
        .map_err(|_| {
            ExternalProviderRuntimeError::Operation("fixture terminal deadline exceeded".into())
        })?
    }
    fn wire(&self) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
        std::fs::read_to_string(self.root.path().join("wire.jsonl"))?
            .lines()
            .map(|line| serde_json::from_str(line).map_err(Into::into))
            .collect()
    }
    async fn close(&self) -> Result<(), tokio::time::error::Elapsed> {
        tokio::time::timeout(BOUND, self.client.shutdown()).await
    }
}
fn text_input(text: &str) -> Result<ProviderTurnInput, InvalidPromptContent> {
    Ok(ProviderTurnInput::Prompt(ProviderSubmission::plain(vec![
        PromptContent::text(text.into())?,
    ])))
}

#[tokio::test]
async fn real_actor_uses_different_injected_results_and_preserves_original_user_history() {
    for (skill, output) in [
        ("first-skill", "first provider"),
        ("second-skill", "second provider"),
    ] {
        let fixture = Fixture::open(
            ScriptedBehavior {
                skill,
                preparation: Preparation::Literal(output),
            },
            "manual",
        )
        .await
        .expect("fixture setup");
        let original = format!("${skill} original λ");
        let submission = ProviderSubmission {
            content: vec![PromptContent::text(original.clone()).expect("text")],
            selected: vec![SelectedSkill {
                invocation_name: SkillInvocationName::new(skill).expect("selection"),
            }],
        };
        fixture
            .submit(ProviderTurnInput::Prompt(submission))
            .await
            .expect("selected prompt");
        assert_eq!(
            fixture.wire().expect("wire receipt")[0]["params"]["prompt"],
            serde_json::json!([{"type":"text","text":output}])
        );
        assert!(fixture.sink.events.lock().expect("events").iter().any(|event| matches!(event, SessionEvent::ItemStarted { item } if item.kind == SessionItemKind::UserMessage && item.text.as_deref() == Some(original.as_str()))));
        fixture.close().await.expect("owned shutdown");
    }
}

#[tokio::test]
async fn stale_selection_resolves_dispatch_immediately_without_prompt_or_turn_start() {
    let mut fixture = Fixture::open(
        ScriptedBehavior {
            skill: "review",
            preparation: Preparation::Preserve,
        },
        "manual",
    )
    .await
    .expect("fixture setup");
    fixture
        .submit(text_input("remove-skill").expect("text"))
        .await
        .expect("remove inventory");
    assert!(
        matches!(fixture.available().await.expect("current view"), SkillView::Available { skills, .. } if skills.is_empty())
    );
    let before = fixture.sink.events.lock().expect("events").len();
    let (dispatch, observation) = tokio::sync::oneshot::channel();
    let input = ProviderTurnInput::Prompt(ProviderSubmission {
        content: vec![PromptContent::text("$review".into()).expect("text")],
        selected: vec![SelectedSkill {
            invocation_name: SkillInvocationName::new("review").expect("name"),
        }],
    });
    let result = tokio::time::timeout(
        BOUND,
        fixture.client.submit_with_approval_dispatch_for_input(
            fixture.session.clone(),
            InputId::generate(),
            input,
            2,
            Some(dispatch),
        ),
    )
    .await
    .expect("closed refusal");
    assert!(matches!(
        result,
        Err(ExternalProviderRuntimeError::Behavior(
            ProviderError::InvalidSelection
        ))
    ));
    assert_eq!(
        observation.await.expect("dispatch report"),
        ProviderPromptDispatchObservation::Refused(ProviderDispatchRefusal::Behavior(
            ProviderError::InvalidSelection
        ))
    );
    assert_eq!(fixture.wire().expect("wire receipt").len(), 1);
    assert_eq!(fixture.sink.events.lock().expect("events").len(), before);
    fixture.close().await.expect("owned shutdown");
}

#[tokio::test]
async fn malformed_semantic_tool_metadata_preserves_real_stream_terminal_and_skills() {
    let fixture = Fixture::open(
        ScriptedBehavior {
            skill: "review",
            preparation: Preparation::Preserve,
        },
        "manual",
    )
    .await
    .expect("fixture setup");
    let outcome = fixture
        .submit(text_input("semantic-error").expect("text"))
        .await
        .expect("ordinary turn stays real");
    assert_eq!(outcome.output, "beforeafter");
    assert_eq!(outcome.stop_reason, StopReason::EndTurn);
    assert_eq!(
        fixture
            .sink
            .events
            .lock()
            .expect("events")
            .iter()
            .filter(|event| matches!(event, SessionEvent::TurnEnded { .. }))
            .count(),
        1
    );
    let input = ProviderTurnInput::Prompt(ProviderSubmission {
        content: vec![PromptContent::text("$review".into()).expect("text")],
        selected: vec![SelectedSkill {
            invocation_name: SkillInvocationName::new("review").expect("name"),
        }],
    });
    fixture
        .submit(input)
        .await
        .expect("unrelated valid skill remains usable");
    fixture.close().await.expect("owned shutdown");
}

#[tokio::test]
async fn compact_requires_manual_marker_and_successful_terminal_and_keeps_chat_usable() {
    for case in [
        "manual",
        "terminal",
        "none",
        "auto",
        "failed",
        "malformed",
        "cancelled",
    ] {
        let fixture = Fixture::open(
            ScriptedBehavior {
                skill: "review",
                preparation: Preparation::Preserve,
            },
            case,
        )
        .await
        .expect("fixture setup");
        let result = fixture.submit(ProviderTurnInput::Compact).await;
        match case {
            "manual" | "terminal" => assert!(result.is_ok(), "{case}: {result:?}"),
            "cancelled" => assert!(matches!(
                result,
                Ok(ExternalProviderPromptOutcome {
                    stop_reason: StopReason::Cancelled,
                    ..
                })
            )),
            _ => assert!(
                matches!(
                    result,
                    Err(ExternalProviderRuntimeError::CompactionUnconfirmed)
                ),
                "{case}: {result:?}"
            ),
        }
        {
            let events = fixture.sink.events.lock().expect("events");
            let promoted = events.iter().enumerate().filter_map(|(index, event)| match event {
                SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item } if matches!(&item.kind, SessionItemKind::ToolCall { tool_kind, .. } if tool_kind == "contextCompaction") => Some((index, item.item_id.as_str())),
                _ => None,
            }).collect::<Vec<_>>();
            if matches!(case, "manual" | "terminal") {
                assert_eq!(promoted.len(), 1, "one confirmed promotion: {case}");
                assert_eq!(promoted[0].1, "compact-tool");
                let after = events.iter().rposition(|event| matches!(event, SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item } if item.kind == SessionItemKind::AgentMessage && item.text.as_deref() == Some("after"))).expect("last provider chunk");
                assert!(
                    promoted[0].0 > after,
                    "promotion waits for actual terminal: {case}"
                );
            } else {
                assert!(promoted.is_empty(), "no confirmation item for {case}");
            }
        }
        if case == "manual" {
            assert!(fixture.sink.events.lock().expect("events").iter().any(|event| matches!(event, SessionEvent::ItemUpdated { item } if matches!(&item.kind, SessionItemKind::ToolCall { tool_kind, status: ToolCallStatus::Completed } if tool_kind == "contextCompaction"))));
        }
        fixture
            .submit(text_input("after compact").expect("text"))
            .await
            .expect("chat remains usable");
        fixture.close().await.expect("owned shutdown");
    }
}

#[tokio::test]
async fn busy_compact_reports_closed_refusal_before_any_second_prompt() {
    let fixture = Fixture::open(
        ScriptedBehavior {
            skill: "review",
            preparation: Preparation::Preserve,
        },
        "manual",
    )
    .await
    .expect("fixture setup");
    let client = Arc::clone(&fixture.client);
    let session = fixture.session.clone();
    let running = tokio::spawn(async move {
        client
            .submit_with_approval_dispatch_for_input(
                session,
                InputId::generate(),
                text_input("hold").expect("held input"),
                1,
                None,
            )
            .await
    });
    tokio::time::timeout(BOUND, async {
        loop {
            if fixture.sink.events.lock().expect("events").iter().any(|event| matches!(event, SessionEvent::ItemStarted { item } if item.text.as_deref() == Some("HOLDING"))) { break; }
            fixture.sink.published.notified().await;
        }
    }).await.expect("held real provider prompt");
    let (dispatch, observation) = tokio::sync::oneshot::channel();
    let compact = tokio::time::timeout(
        BOUND,
        fixture.client.submit_with_approval_dispatch_for_input(
            fixture.session.clone(),
            InputId::generate(),
            ProviderTurnInput::Compact,
            2,
            Some(dispatch),
        ),
    )
    .await
    .expect("busy refusal");
    let report = tokio::time::timeout(BOUND, observation)
        .await
        .expect("dispatch closes");
    let requests = fixture.wire().expect("wire receipt").len();
    fixture
        .client
        .cancel_active_prompt(fixture.session.clone())
        .await
        .expect("cancel held prompt");
    tokio::time::timeout(BOUND, running)
        .await
        .expect("held prompt settles")
        .expect("prompt task")
        .expect("cancelled terminal");
    fixture.close().await.expect("owned shutdown");
    assert!(matches!(
        compact,
        Err(ExternalProviderRuntimeError::LocalBusy)
    ));
    assert_eq!(
        report,
        Ok(ProviderPromptDispatchObservation::Refused(
            ProviderDispatchRefusal::Busy
        ))
    );
    assert_eq!(requests, 1);
}
