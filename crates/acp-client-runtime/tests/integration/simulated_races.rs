//! Turmoil scenarios (design note §11.8, composed async orders): the real
//! client over a duplex pair to an in-process agent, on Turmoil's
//! deterministic runtime. The client's named points (test-support) are
//! forwarded to Turmoil's barriers; nothing is timed.

#![expect(
    clippy::expect_used,
    reason = "A connection or session the scenario cannot set up must fail it at once."
)]

mod actor_exit_handoff;
mod held_session_mailbox;
mod scripted_line_agent;
mod superseded_restore;

use acp_client_runtime::sim_points::ClientSimPoint;
use acp_client_runtime::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderRuntimeError,
    HistoryReplayFuture, InteractionFuture, InteractionPort, ProviderTurnInput,
    RefusedApprovalOffer, SessionEventSink,
};
use scripted_line_agent::ScriptedLineAgent;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use turmoil::barriers::{Barrier, Reaction};

/// Forwards every named point to Turmoil's barriers, on the simulation's
/// own thread.
fn forward_named_points_to_turmoil() {
    acp_client_runtime::sim_points::install(|point| Box::pin(turmoil::barriers::trigger(point)));
}

/// An interaction port that never sees a request in these scenarios.
struct NoInteraction;

impl InteractionPort for NoInteraction {
    type Context = ();
    type OperationId = u64;
    fn operation_id(_context: &Self::Context) -> Self::OperationId {
        1
    }
    fn binding_retirement(_context: &Self::Context) -> CancellationToken {
        CancellationToken::new()
    }
    fn request_approval(
        &self,
        _context: (),
        _request: session_event_model::ApprovalRequest,
        _turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        Box::pin(async { ApprovalPortOutcome::Cancelled })
    }
    fn request_question(
        &self,
        _context: (),
        _request: session_event_model::QuestionRequest,
        _turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, session_event_model::QuestionResponse> {
        Box::pin(async { session_event_model::QuestionResponse::Cancelled })
    }
    fn record_refusal(
        &self,
        _context: (),
        _refusal: RefusedApprovalOffer,
    ) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
    fn cancel_all(&self, _context: (), _reason: &'static str) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
    fn cancel_retired(&self) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// Accepts every event and keeps none.
struct DiscardingSink;

impl SessionEventSink for DiscardingSink {
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

    fn publish(
        &self,
        _session_id: &str,
        _event: session_event_model::SessionEvent,
    ) -> Result<(), EventSinkClosed> {
        Ok(())
    }
}

/// Oracle: §12.1's EndingTurn, written by hand: a close that finds the
/// session's turn end still held (the Host holds it until the prompt's
/// settlement is written) sends session/close only after that turn end is
/// released. The agent reads no session/close while it is held.
#[test]
fn a_close_frees_the_session_only_after_its_held_turn_end() {
    forward_named_points_to_turmoil();
    let mut simulation = turmoil::Builder::new().build();
    simulation.client("router", async move {
        // Arrange: a session whose answered prompt still holds its turn end.
        let mut close_waits = Barrier::build(Reaction::Noop, |point: &ClientSimPoint| {
            *point == ClientSimPoint::CloseTurnWait
        });
        let (client_end, agent_end) = tokio::io::duplex(64 * 1024);
        let (agent, agent_task) = ScriptedLineAgent::start(agent_end);
        let client = Arc::new(
            AgentSessionClient::initialize_in_process(
                client_end,
                agent_task,
                Arc::new(NoInteraction),
                Arc::new(DiscardingSink),
                Arc::new(super::test_behavior::TestProviderBehavior),
            )
            .await
            .expect("the connection initializes"),
        );
        let session = client
            .create_session(std::path::PathBuf::from("/scenario"))
            .await
            .expect("session/new");
        let text: session_event_model::PromptContent =
            serde_json::from_value(serde_json::json!({"type": "text", "text": "work"}))
                .expect("text block");
        let (prompted, turn_end) = client
            .submit_holding_turn_end(
                session.clone(),
                session_event_model::InputId::generate(),
                ProviderTurnInput::Prompt(session_event_model::ProviderSubmission::plain(vec![
                    text,
                ])),
                (),
                None,
                None,
            )
            .await;
        prompted.expect("the agent answers the prompt");
        let turn_end = turn_end.expect("the turn end is held");

        // Act: the close reaches its turn wait, or (a wrong build) the
        // agent reads session/close first.
        let closing = tokio::spawn({
            let client = Arc::clone(&client);
            let session = session.clone();
            async move { client.close_session(session).await }
        });
        let close_waited = tokio::select! {
            triggered = close_waits.wait() => triggered.is_some(),
            () = agent.received("session/close") => false,
        };
        let methods_while_held = agent.methods();
        drop(turn_end);
        let closed = closing.await.expect("close task");
        let methods = agent.methods();

        // Assert
        assert!(
            close_waited,
            "session/close was sent with the turn end held: {methods:?}"
        );
        assert!(
            !methods_while_held
                .iter()
                .any(|method| method == "session/close"),
            "{methods_while_held:?}"
        );
        assert!(closed.is_ok(), "{closed:?}");
        assert_eq!(methods.last().map(String::as_str), Some("session/close"));
        Ok(())
    });
    simulation.run().expect("the scenario completes");
}

/// The scenario's client and agent, with one live session.
async fn connected() -> (
    Arc<AgentSessionClient<NoInteraction>>,
    ScriptedLineAgent,
    String,
) {
    let (client_end, agent_end) = tokio::io::duplex(64 * 1024);
    let (agent, agent_task) = ScriptedLineAgent::start(agent_end);
    let client = Arc::new(
        AgentSessionClient::initialize_in_process(
            client_end,
            agent_task,
            Arc::new(NoInteraction),
            Arc::new(DiscardingSink),
            Arc::new(super::test_behavior::TestProviderBehavior),
        )
        .await
        .expect("the connection initializes"),
    );
    let session = client
        .create_session(std::path::PathBuf::from("/scenario"))
        .await
        .expect("session/new");
    (client, agent, session)
}

/// Oracle: §12.1's EndingTurn and the session table (Closing × prompt
/// requested: refused "session is closing", so the message is not sent)
/// close the race logged in out-of-scope.md, a prompt racing a finished
/// close. A close requested after a prompt's turn is open on the cell, but
/// before its command reaches the connection, waits for that turn and can't
/// finish while the prompt is held there. Once the prompt goes on it is
/// refused with the closing answer, never left with no actor; the agent
/// never reads it, and the close then completes.
#[test]
fn a_close_cannot_overtake_a_prompt_whose_turn_is_open() {
    forward_named_points_to_turmoil();
    let mut simulation = turmoil::Builder::new().build();
    simulation.client("router", async move {
        // Arrange
        let mut prompt_open = Barrier::build(Reaction::Suspend, |point: &ClientSimPoint| {
            *point == ClientSimPoint::PromptTurnOpen
        });
        let mut close_waits = Barrier::build(Reaction::Noop, |point: &ClientSimPoint| {
            *point == ClientSimPoint::CloseTurnWait
        });
        let (client, agent, session) = connected().await;
        let text: session_event_model::PromptContent =
            serde_json::from_value(serde_json::json!({"type": "text", "text": "work"}))
                .expect("text block");

        // Act: the prompt is held with its turn open; a close is requested.
        let prompting = tokio::spawn({
            let client = Arc::clone(&client);
            let session = session.clone();
            async move {
                let (prompted, turn_end) = client
                    .submit_holding_turn_end(
                        session,
                        session_event_model::InputId::generate(),
                        ProviderTurnInput::Prompt(session_event_model::ProviderSubmission::plain(
                            vec![text],
                        )),
                        (),
                        None,
                        None,
                    )
                    .await;
                drop(turn_end);
                prompted
            }
        });
        let held_prompt = prompt_open.wait().await.expect("the prompt's turn opens");
        let mut closing = tokio::spawn({
            let client = Arc::clone(&client);
            let session = session.clone();
            async move { client.close_session(session).await }
        });
        let close_waited = tokio::select! {
            triggered = close_waits.wait() => Ok(triggered.is_some()),
            closed = &mut closing => Err(closed),
        };
        let methods_while_held = agent.methods();
        drop(held_prompt);
        let prompted = prompting.await.expect("prompt task");
        // A close that already finished was taken by the select above.
        let closed_cleanly = match &close_waited {
            Ok(_) => closing.await.expect("close task").is_ok(),
            Err(early) => matches!(early, Ok(Ok(()))),
        };
        let methods = agent.methods();

        // Assert
        assert_eq!(
            close_waited.as_ref().ok(),
            Some(&true),
            "the close finished while the prompt's turn was open: {close_waited:?}"
        );
        assert!(
            !methods_while_held
                .iter()
                .any(|method| method == "session/close" || method == "session/prompt"),
            "{methods_while_held:?}"
        );
        assert!(
            matches!(
                &prompted,
                Err(ExternalProviderRuntimeError::LocalBusy { answer })
                    if answer == "session is closing; wait for it"
            ),
            "{prompted:?}"
        );
        assert!(closed_cleanly, "the close completes");
        assert!(
            !methods.iter().any(|method| method == "session/prompt"),
            "{methods:?}"
        );
        assert_eq!(methods.last().map(String::as_str), Some("session/close"));
        Ok(())
    });
    simulation.run().expect("the scenario completes");
}
