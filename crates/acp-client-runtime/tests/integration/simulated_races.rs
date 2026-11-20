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
mod restore_fast_off;
mod scripted_line_agent;
mod superseded_restore;

use crate::acp_owner_fixture::FixtureClient as AgentSessionClient;
use acp_client_runtime::sim_points::ClientSimPoint;
use acp_client_runtime::{
    ApprovalPortOutcome, EventSinkClosed, ExternalProviderRuntimeError, HistoryReplayFuture,
    InteractionFuture, InteractionPort, ProviderTurnInput, RefusedApprovalOffer, SessionEventSink,
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

/// T7. Close requests cancellation and waits while the provider holds the
/// running prompt's terminal response.
#[test]
fn a_close_frees_the_session_only_after_its_held_turn_end() {
    let mut simulation = turmoil::Builder::new().build();
    simulation.client("router", async move {
        let (client, agent, session) = connected().await;
        agent.hold_next_prompt();
        let prompting = tokio::spawn({
            let client = client.clone();
            let session = session.clone();
            async move { fixture_prompt(&client, session).await }
        });
        agent.wait_for_held_prompt().await;
        let closing = tokio::spawn({
            let client = client.clone();
            async move { client.close_session(session).await }
        });
        agent.received("session/cancel").await;

        assert!(
            !agent
                .methods()
                .iter()
                .any(|method| method == "session/close")
        );
        agent.release_held_prompt().await;
        prompting
            .await
            .expect("prompt task")
            .expect("prompt settled");
        closing.await.expect("close task").expect("close settled");
        assert_eq!(
            agent.methods().last().map(String::as_str),
            Some("session/close")
        );
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
            Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH)),
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

/// T2 and T7. A close can retire a run before its dispatch mark settles.
/// The old mark then releases no authorization and the prompt is known unsent.
#[test]
fn a_close_during_a_held_dispatch_mark_never_sends_its_prompt() {
    forward_named_points_to_turmoil();
    let mut simulation = turmoil::Builder::new().build();
    simulation.client("router", async move {
        let mut mark_pause = Barrier::build(Reaction::Suspend, |point: &ClientSimPoint| {
            *point == ClientSimPoint::DispatchMarkOffered
        });
        let (client, agent, session) = connected().await;
        let mut prompting = tokio::spawn({
            let client = client.clone();
            let session = session.clone();
            async move { fixture_prompt(&client, session).await }
        });
        let mark = tokio::select! {
            mark = mark_pause.wait() => mark.expect("prompt reached held mark"),
            result = &mut prompting => panic!("prompt settled before the mark barrier: {result:?}"),
        };
        client
            .close_session(session)
            .await
            .expect("close retires the run");
        assert!(
            !agent
                .methods()
                .iter()
                .any(|method| method == "session/prompt")
        );
        drop(mark);
        let result = prompting.await.expect("prompt task");
        assert!(result.is_err(), "no late prompt completes: {result:?}");
        assert_eq!(
            agent.methods().last().map(String::as_str),
            Some("session/close")
        );
        Ok(())
    });
    simulation.run().expect("the scenario completes");
}

async fn fixture_prompt(
    client: &AgentSessionClient<NoInteraction>,
    session: String,
) -> Result<acp_client_runtime::ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
    let text = session_event_model::PromptContent::text("work".to_owned()).expect("fixture text");
    client
        .submit_with_approval_dispatch_for_input(
            session,
            session_event_model::InputId::generate(),
            ProviderTurnInput::Prompt(session_event_model::ProviderSubmission::plain(vec![text])),
            (),
            None,
        )
        .await
}
