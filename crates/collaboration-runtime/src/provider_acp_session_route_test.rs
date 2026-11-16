use super::*;
use codex_acp_adapter::bounded_acp_output;
use collaboration_contracts::{
    CommandFuture, HubEvent, HubFuture, HubSessionSummary, QueueInputCommand, QueuedInputOutcome,
    QueuedSessionInput, SessionEventAttachment, SessionTurnHandle, SetSessionSettingCommand,
    SkillViewAttachment,
};
use session_event_model::{CapabilityReport, InputId, StopReason, TurnOutcome};
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// What each busy-prompt command answers; each is taken once, and an
/// unscripted command answers Unsupported.
#[derive(Default)]
struct BusyScript {
    steer: Option<Result<SessionSteerOutcome, CommandFailure>>,
    /// The answer to a second steer, once the first answer is taken.
    second_steer: Option<Result<SessionSteerOutcome, CommandFailure>>,
    prompt: Option<Result<SessionTurnHandle, CommandFailure>>,
    /// The answer to a second prompt, once the first answer is taken.
    second_prompt: Option<Result<SessionTurnHandle, CommandFailure>>,
    queue_add: Option<Result<QueuedSessionInput, CommandFailure>>,
    queue_outcome: Option<Result<QueuedInputOutcome, CommandFailure>>,
}

/// What the route asked the command port to do.
#[derive(Default)]
struct RecordedCalls {
    steer_expected_turns: Vec<String>,
    prompts: usize,
    queue_adds: usize,
    queue_outcomes: usize,
    cancels: usize,
}

/// Records the commands and answers just enough of the hub for the ACP route.
/// The hub's event line is a broadcast the test publishes on.
struct RecordingSessionBackend {
    session: SessionRef,
    approver: Identity,
    created: Mutex<Vec<CreateSessionCommand>>,
    running_turn: Option<String>,
    script: Mutex<BusyScript>,
    calls: Mutex<RecordedCalls>,
    events: tokio::sync::broadcast::Sender<HubEvent>,
}

impl RecordingSessionBackend {
    fn new(endpoint: SessionEndpointRef) -> Result<Arc<Self>, serde_json::Error> {
        Self::with_script(endpoint, None, BusyScript::default())
    }

    fn with_script(
        endpoint: SessionEndpointRef,
        running_turn: Option<&str>,
        script: BusyScript,
    ) -> Result<Arc<Self>, serde_json::Error> {
        let session: SessionRef = serde_json::from_value(json!({
            "endpoint":endpoint,
            "sessionId":"19e49a31-daa3-428c-b985-e0c7373a89ed"
        }))?;
        let (events, _first_receiver) = tokio::sync::broadcast::channel::<HubEvent>(64);
        Ok(Arc::new(Self {
            session,
            approver: acp_actor()?,
            created: Mutex::new(Vec::new()),
            running_turn: running_turn.map(str::to_owned),
            script: Mutex::new(script),
            calls: Mutex::new(RecordedCalls::default()),
            events,
        }))
    }

    fn publish_turn_ended(&self, turn_id: &str) {
        let _receivers = self.events.send(HubEvent {
            sequence: 100,
            event: SessionEvent::TurnEnded {
                turn_id: turn_id.to_owned(),
                outcome: TurnOutcome::Ended {
                    stop_reason: StopReason::EndTurn,
                    local_cause: None,
                },
            },
        });
    }
}

fn acp_actor() -> Result<Identity, serde_json::Error> {
    serde_json::from_value(json!({"kind":"human","humanId":"owner"}))
}

impl SessionCommandPort for RecordingSessionBackend {
    fn create(&self, command: CreateSessionCommand) -> CommandFuture<'_, SessionRef> {
        self.created.lock().expect("test lock").push(command);
        let session = self.session.clone();
        Box::pin(async move { Ok(session) })
    }
    fn load_session(&self, _: SessionTargetCommand) -> CommandFuture<'_, ()> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn resume_session(&self, _: SessionTargetCommand) -> CommandFuture<'_, ()> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn prompt(&self, _: PromptSessionCommand) -> CommandFuture<'_, SessionTurnHandle> {
        self.calls.lock().expect("test lock").prompts += 1;
        let answer = {
            let mut script = self.script.lock().expect("test lock");
            script.prompt.take().or_else(|| script.second_prompt.take())
        };
        Box::pin(async { answer.unwrap_or(Err(CommandFailure::Unsupported)) })
    }
    fn compact(&self, _: SessionTargetCommand) -> CommandFuture<'_, SessionTurnHandle> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn steer(&self, command: SteerSessionCommand) -> CommandFuture<'_, SessionSteerOutcome> {
        self.calls
            .lock()
            .expect("test lock")
            .steer_expected_turns
            .push(command.expected_turn_id);
        let answer = {
            let mut script = self.script.lock().expect("test lock");
            script.steer.take().or_else(|| script.second_steer.take())
        };
        Box::pin(async { answer.unwrap_or(Err(CommandFailure::Unsupported)) })
    }
    fn queue_add(&self, _: QueueInputCommand) -> CommandFuture<'_, QueuedSessionInput> {
        self.calls.lock().expect("test lock").queue_adds += 1;
        let answer = self.script.lock().expect("test lock").queue_add.take();
        Box::pin(async { answer.unwrap_or(Err(CommandFailure::Unsupported)) })
    }
    fn queue_outcome(&self, _: SessionRef, _: InputId) -> CommandFuture<'_, QueuedInputOutcome> {
        self.calls.lock().expect("test lock").queue_outcomes += 1;
        let answer = self.script.lock().expect("test lock").queue_outcome.take();
        Box::pin(async { answer.unwrap_or(Err(CommandFailure::Unsupported)) })
    }
    fn queue_list(&self, _: SessionTargetCommand) -> CommandFuture<'_, Vec<QueuedSessionInput>> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn queue_cancel(&self, _: SessionTargetCommand, _: String) -> CommandFuture<'_, ()> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn cancel(&self, _: SessionTargetCommand) -> CommandFuture<'_, ()> {
        self.calls.lock().expect("test lock").cancels += 1;
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn close(&self, _: SessionTargetCommand) -> CommandFuture<'_, ()> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
    fn set_setting(&self, _: SetSessionSettingCommand) -> CommandFuture<'_, ()> {
        Box::pin(async { Err(CommandFailure::Unsupported) })
    }
}

impl SessionEventHub for RecordingSessionBackend {
    fn skill_view(&self, _: SessionRef, _: PathBuf) -> HubFuture<'_, SkillViewAttachment> {
        Box::pin(async { Err(collaboration_contracts::SessionEventHubError::Unavailable) })
    }
    fn attach(&self, _: SessionRef) -> HubFuture<'_, SessionEventAttachment> {
        let receiver = self.events.subscribe();
        let mut snapshot = vec![HubEvent {
            sequence: 1,
            event: SessionEvent::CapabilitiesChanged {
                capabilities: CapabilityReport::default(),
            },
        }];
        if let Some(turn_id) = &self.running_turn {
            snapshot.push(HubEvent {
                sequence: 2,
                event: SessionEvent::TurnStarted {
                    turn_id: turn_id.clone(),
                    input_id: InputId::generate(),
                },
            });
        }
        Box::pin(async move {
            Ok(SessionEventAttachment {
                snapshot,
                receiver,
                epoch: 0,
            })
        })
    }
    fn state(&self, _: SessionRef) -> HubFuture<'_, SessionState> {
        let state = if self.running_turn.is_some() {
            SessionState::Running
        } else {
            SessionState::Idle
        };
        Box::pin(async move { Ok(state) })
    }
    fn sessions(&self, _: SessionEndpointRef) -> HubFuture<'_, Vec<HubSessionSummary>> {
        let summary = HubSessionSummary {
            session: self.session.clone(),
            approver: self.approver.clone(),
            working_directory: PathBuf::from("/"),
            updated_at_seconds: 1_700_000_000,
            preview: String::new(),
            name: None,
            model: None,
            mode: None,
            state: SessionState::Idle,
        };
        Box::pin(async move { Ok(vec![summary]) })
    }
}

/// Oracle: spec R1 "Those doors ask for auto by name". ACP `session/new` has no
/// field for a permission mode, so the Router's ACP socket door names `auto` itself.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn session_new_asks_for_auto() -> Result<(), Box<dyn std::error::Error>> {
    // Arrange: the route over an in-memory frame pair and a recording backend.
    let directory = tempfile::tempdir()?;
    let endpoint: SessionEndpointRef = serde_json::from_value(json!({
        "serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89",
        "endpointId":"claude-local"
    }))?;
    let backend = RecordingSessionBackend::new(endpoint.clone())?;
    let closed = CancellationToken::new();
    let (frame_sender, frame_receiver) = bounded_acp_output(closed.clone());
    let (output_sender, mut output_receiver) = bounded_acp_output(closed.clone());
    let route = Box::new(ProviderAcpSessionRoute::new(
        endpoint,
        Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
        Arc::clone(&backend) as Arc<dyn SessionEventHub>,
    ));
    let served = tokio::spawn(route.run(
        codex_acp_adapter::AcpRouterChannels {
            input: frame_receiver,
            output: output_sender,
            closed: closed.clone(),
        },
        AcpConnectionContext {
            actor: Some(acp_actor()?),
            client_profile: None,
            client_supports_elicitation_form: false,
        },
    ));

    // Act: one valid session/new request, then wait for its response.
    frame_sender
        .send(json!({"jsonrpc":"2.0","id":7,"method":"session/new",
            "params":{"cwd":directory.path(),"mcpServers":[]}}))
        .await?;
    let reply = output_receiver
        .recv()
        .await
        .ok_or("route closed before replying")?;

    // Assert: the create succeeded and asked for mode auto, nothing else set.
    assert_eq!(reply["id"], 7, "{}", *reply);
    assert!(reply.get("error").is_none(), "{}", *reply);
    {
        let created = backend.created.lock().expect("test lock");
        assert_eq!(created.len(), 1);
        assert_eq!(
            created[0].settings,
            SessionSettingsCommand {
                model: None,
                mode: Some("auto".into()),
                effort: None,
            }
        );
    }
    closed.cancel();
    served.await??;
    Ok(())
}

// Waits here are bounded by the test runner (.config/nextest.toml), never by a
// clock in test code (AGENTS.md "Time").

/// Scheduler turns the route gets before a check that it has not answered.
const QUIET_TURNS: usize = 256;

/// The route served over in-memory frames, with a session already created.
struct BusyRouteHarness {
    backend: Arc<RecordingSessionBackend>,
    frames: codex_acp_adapter::AcpOutputSender,
    replies: tokio::sync::mpsc::Receiver<codex_acp_adapter::QueuedAcpFrame>,
    closed: CancellationToken,
    served: tokio::task::JoinHandle<std::io::Result<()>>,
    _directory: tempfile::TempDir,
}

impl BusyRouteHarness {
    async fn start(
        running_turn: &str,
        script: BusyScript,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::start_in_state(Some(running_turn), script).await
    }

    /// The same harness on a session the hub reports idle.
    async fn start_idle(script: BusyScript) -> Result<Self, Box<dyn std::error::Error>> {
        Self::start_in_state(None, script).await
    }

    async fn start_in_state(
        running_turn: Option<&str>,
        script: BusyScript,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let endpoint: SessionEndpointRef = serde_json::from_value(json!({
            "serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89",
            "endpointId":"claude-local"
        }))?;
        let backend = RecordingSessionBackend::with_script(endpoint.clone(), running_turn, script)?;
        let closed = CancellationToken::new();
        let (frames, frame_receiver) = bounded_acp_output(closed.clone());
        let (output_sender, replies) = bounded_acp_output(closed.clone());
        let route = Box::new(ProviderAcpSessionRoute::new(
            endpoint,
            Arc::clone(&backend) as Arc<dyn SessionCommandPort>,
            Arc::clone(&backend) as Arc<dyn SessionEventHub>,
        ));
        let served = tokio::spawn(route.run(
            codex_acp_adapter::AcpRouterChannels {
                input: frame_receiver,
                output: output_sender,
                closed: closed.clone(),
            },
            AcpConnectionContext {
                actor: Some(acp_actor()?),
                client_profile: None,
                client_supports_elicitation_form: false,
            },
        ));
        let mut harness = Self {
            backend,
            frames,
            replies,
            closed,
            served,
            _directory: directory,
        };
        let created = harness
            .request(json!({"jsonrpc":"2.0","id":1,"method":"session/new",
                "params":{"cwd":harness._directory.path(),"mcpServers":[]}}))
            .await?;
        assert!(created.get("error").is_none(), "{created}");
        Ok(harness)
    }

    /// Sends a request and returns the response carrying its id.
    async fn request(&mut self, request: Value) -> Result<Value, Box<dyn std::error::Error>> {
        let id = request["id"].clone();
        self.frames.send(request).await?;
        self.reply_for(&id).await
    }

    async fn send_prompt(&mut self, id: u64) -> Result<(), Box<dyn std::error::Error>> {
        self.frames
            .send(
                json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":{
                    "sessionId":self.backend.session.session_id.as_str(),
                    "prompt":[{"type":"text","text":"next thing"}]
                }}),
            )
            .await?;
        Ok(())
    }

    /// The next response with this id; notifications are skipped.
    async fn reply_for(&mut self, id: &Value) -> Result<Value, Box<dyn std::error::Error>> {
        loop {
            let frame = self.replies.recv().await.ok_or("route closed")?;
            if frame.get("id") == Some(id) && frame.get("method").is_none() {
                return Ok((*frame).clone());
            }
        }
    }

    /// True when a response with this id is already there after the route has
    /// had its scheduler turns; notifications seen on the way are skipped.
    async fn replies_without_waiting(&mut self, id: &Value) -> bool {
        for _ in 0..QUIET_TURNS {
            tokio::task::yield_now().await;
        }
        while let Ok(frame) = self.replies.try_recv() {
            if frame.get("id") == Some(id) && frame.get("method").is_none() {
                return true;
            }
        }
        false
    }

    /// Yields until the command port has recorded what the test waits for.
    async fn until_recorded(
        &self,
        recorded: impl Fn(&RecordedCalls) -> bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        while !recorded(&self.backend.calls.lock().expect("test lock")) {
            tokio::task::yield_now().await;
        }
        Ok(())
    }

    async fn stop(self) -> Result<(), Box<dyn std::error::Error>> {
        self.closed.cancel();
        self.served.await??;
        Ok(())
    }
}

fn queued_input() -> QueuedSessionInput {
    QueuedSessionInput {
        input_id: InputId::generate().into(),
        position: 1,
        preview: "next thing".into(),
    }
}

/// Oracle: D1 R12 — a prompt to a session in a turn steers into that turn and
/// answers when that turn ends; it is not refused as busy, and nothing is cancelled.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn busy_prompt_steers_and_answers_at_the_running_turn_end()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange: a session running turn t1 whose agent can steer.
    let mut harness = BusyRouteHarness::start(
        "t1",
        BusyScript {
            steer: Some(Ok(SessionSteerOutcome::Injected {
                turn_id: "t1".into(),
            })),
            ..BusyScript::default()
        },
    )
    .await?;

    // Act: prompt while t1 runs; nothing answers until t1 ends.
    harness.send_prompt(2).await?;
    harness
        .until_recorded(|calls| !calls.steer_expected_turns.is_empty())
        .await?;
    let answered_early = harness.replies_without_waiting(&json!(2)).await;
    // Asserted here: a reply already taken can't be waited for again.
    assert!(!answered_early, "the prompt answered before its turn ended");
    harness.backend.publish_turn_ended("t1");
    let reply = harness.reply_for(&json!(2)).await?;

    // Assert: one steer at t1, no other command, and a stop reason at the turn end.
    assert_eq!(reply["result"]["stopReason"], "end_turn", "{reply}");
    {
        let calls = harness.backend.calls.lock().expect("test lock");
        assert_eq!(calls.steer_expected_turns, vec!["t1".to_owned()]);
        assert_eq!((calls.prompts, calls.queue_adds, calls.cancels), (0, 0, 0));
    }
    harness.stop().await
}

/// Oracle: D1 R12 — an agent that cannot steer gets the prompt queued; the
/// answer comes at the end of the turn the queued input started.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn busy_prompt_queues_when_steering_unsupported() -> Result<(), Box<dyn std::error::Error>> {
    // Arrange: steering unsupported; the queued input starts turn t2.
    let mut harness = BusyRouteHarness::start(
        "t1",
        BusyScript {
            steer: Some(Err(CommandFailure::Unsupported)),
            queue_add: Some(Ok(queued_input())),
            queue_outcome: Some(Ok(QueuedInputOutcome::Started {
                turn_id: "t2".into(),
            })),
            ..BusyScript::default()
        },
    )
    .await?;

    // Act: prompt, wait for the queue to report, then end t2.
    harness.send_prompt(2).await?;
    harness
        .until_recorded(|calls| calls.queue_outcomes > 0)
        .await?;
    let answered_early = harness.replies_without_waiting(&json!(2)).await;
    // Asserted here: a reply already taken can't be waited for again.
    assert!(!answered_early, "the prompt answered before t2 ended");
    harness.backend.publish_turn_ended("t2");
    let reply = harness.reply_for(&json!(2)).await?;

    // Assert: one queue_add, an answer only at t2's end, nothing cancelled.
    assert_eq!(reply["result"]["stopReason"], "end_turn", "{reply}");
    {
        let calls = harness.backend.calls.lock().expect("test lock");
        assert_eq!(calls.queue_adds, 1);
        assert_eq!((calls.prompts, calls.cancels), (0, 0));
    }
    harness.stop().await
}

/// Oracle: D1 R12 — the running turn ended before the steer landed, so
/// nothing was injected and the message goes as a fresh prompt exactly once.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn busy_prompt_whose_turn_just_ended_prompts() -> Result<(), Box<dyn std::error::Error>> {
    // Arrange: steer answers PromptRequired; the prompt starts turn t3.
    let mut harness = BusyRouteHarness::start(
        "t1",
        BusyScript {
            steer: Some(Ok(SessionSteerOutcome::PromptRequired)),
            prompt: Some(Ok(SessionTurnHandle {
                turn_id: "t3".into(),
            })),
            ..BusyScript::default()
        },
    )
    .await?;

    // Act: prompt, wait for the fresh prompt, then end t3.
    harness.send_prompt(2).await?;
    harness.until_recorded(|calls| calls.prompts > 0).await?;
    harness.backend.publish_turn_ended("t3");
    let reply = harness.reply_for(&json!(2)).await?;

    // Assert: exactly one prompt, no queue, and the answer is t3's.
    assert_eq!(reply["result"]["stopReason"], "end_turn", "{reply}");
    {
        let calls = harness.backend.calls.lock().expect("test lock");
        assert_eq!(calls.prompts, 1);
        assert_eq!((calls.queue_adds, calls.cancels), (0, 0));
    }
    harness.stop().await
}

/// Oracle: D1 R12 — a queued prompt that is never sent answers with an error
/// that names why, instead of waiting forever.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn queued_prompt_not_sent_answers_with_the_reason() -> Result<(), Box<dyn std::error::Error>>
{
    // Arrange: steering unsupported; the queue reports the input was never sent.
    let mut harness = BusyRouteHarness::start(
        "t1",
        BusyScript {
            steer: Some(Err(CommandFailure::Unsupported)),
            queue_add: Some(Ok(queued_input())),
            queue_outcome: Some(Ok(QueuedInputOutcome::NotSent {
                reason: "session closed".into(),
            })),
            ..BusyScript::default()
        },
    )
    .await?;

    // Act
    harness.send_prompt(2).await?;
    let reply = harness.reply_for(&json!(2)).await?;

    // Assert: an error response carrying the reason.
    let message = reply["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("not sent: session closed"), "{reply}");
    let cancels = harness.backend.calls.lock().expect("test lock").cancels;
    assert_eq!(cancels, 0);
    harness.stop().await
}

/// Oracle: D1 R12, "no Busy" — the hub read the session idle, but a turn
/// started before the prompt landed, so the agent answered busy. The prompt is
/// placed as a busy one (here: its one retry meets the turn again, so it
/// queues) and answers when the turn its queued input started ends; it is never
/// refused as busy.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn idle_prompt_that_meets_a_new_turn_is_placed_never_busy()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange: an idle session whose prompts meet a running turn twice.
    let mut harness = BusyRouteHarness::start_idle(BusyScript {
        prompt: Some(Err(CommandFailure::Busy)),
        second_prompt: Some(Err(CommandFailure::Busy)),
        queue_add: Some(Ok(queued_input())),
        queue_outcome: Some(Ok(QueuedInputOutcome::Started {
            turn_id: "t2".into(),
        })),
        ..BusyScript::default()
    })
    .await?;

    // Act
    harness.send_prompt(2).await?;
    harness
        .until_recorded(|calls| calls.queue_outcomes > 0)
        .await?;
    harness.backend.publish_turn_ended("t2");
    let reply = harness.reply_for(&json!(2)).await?;

    // Assert: never busy; queued once after the race, answered at t2's end.
    assert!(
        reply.get("error").is_none(),
        "never refused as busy: {reply}"
    );
    assert_eq!(reply["result"]["stopReason"], "end_turn", "{reply}");
    {
        let calls = harness.backend.calls.lock().expect("test lock");
        assert_eq!((calls.prompts, calls.queue_adds, calls.cancels), (2, 1, 0));
    }
    harness.stop().await
}

/// Oracle: D1 R12 and V22b, "forced expected-turn mismatch" — the steer names
/// the running turn the route read, but the command port finds the turn has
/// changed and answers busy once. The route reads the turn once more and
/// places the message with exactly one submission (here the second steer is
/// injected), never busy and never cancelling the turn.
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn steer_whose_expected_turn_changed_is_placed_once_after_one_re_read()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange: the first steer meets a changed turn; the second is taken.
    let mut harness = BusyRouteHarness::start(
        "t1",
        BusyScript {
            steer: Some(Err(CommandFailure::Busy)),
            second_steer: Some(Ok(SessionSteerOutcome::Injected {
                turn_id: "t1".into(),
            })),
            ..BusyScript::default()
        },
    )
    .await?;

    // Act
    harness.send_prompt(2).await?;
    harness
        // Whatever the route does next: a second steer, a prompt or a queue_add.
        .until_recorded(|calls| {
            calls.steer_expected_turns.len() >= 2 || calls.prompts > 0 || calls.queue_adds > 0
        })
        .await?;
    harness.backend.publish_turn_ended("t1");
    let reply = harness.reply_for(&json!(2)).await?;

    // Assert: one re-read, one submission, no other command, no busy answer.
    assert!(
        reply.get("error").is_none(),
        "never refused as busy: {reply}"
    );
    assert_eq!(reply["result"]["stopReason"], "end_turn", "{reply}");
    {
        let calls = harness.backend.calls.lock().expect("test lock");
        assert_eq!(
            calls.steer_expected_turns,
            vec!["t1".to_owned(), "t1".to_owned()]
        );
        assert_eq!((calls.prompts, calls.queue_adds, calls.cancels), (0, 0, 0));
    }
    harness.stop().await
}
