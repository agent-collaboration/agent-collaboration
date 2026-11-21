//! An ended run is published before a later run activates.

use super::NoInteraction;
use super::scripted_line_agent::ScriptedLineAgent;
use crate::acp_owner_fixture::FixtureClient as AgentSessionClient;
use acp_client_runtime::sim_points::ClientSimPoint;
use acp_client_runtime::{
    EventSinkClosed, ExternalProviderRuntimeError, GenerationPublication, HistoryReplayFuture,
    SessionEventSink,
};
use provider_session_machines::SessionGeneration;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use turmoil::barriers::{Barrier, Reaction};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RunFact {
    Unloaded(u64),
    Activated(u64),
}

#[derive(Default)]
pub(super) struct RunCheckingSink {
    accepted: Mutex<Vec<RunFact>>,
    changed: tokio::sync::Notify,
}

impl RunCheckingSink {
    fn accepted(&self) -> Vec<RunFact> {
        self.accepted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) async fn unloaded_offered(&self) {
        loop {
            let changed = self.changed.notified();
            if self
                .accepted()
                .iter()
                .any(|fact| matches!(fact, RunFact::Unloaded(_)))
            {
                return;
            }
            changed.await;
        }
    }
}

impl SessionEventSink for RunCheckingSink {
    fn publish_skill_view(
        &self,
        _session: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), EventSinkClosed> {
        Ok(())
    }

    fn begin_history_replay(&self, _session: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    fn publish(
        &self,
        _session: &str,
        _event: session_event_model::SessionEvent,
    ) -> Result<(), EventSinkClosed> {
        Ok(())
    }

    fn publish_in(
        &self,
        _session: &str,
        generation: SessionGeneration,
        event: session_event_model::SessionEvent,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        use session_event_model::{SessionEvent, SessionState};
        let fact = match event {
            SessionEvent::StateChanged {
                state: SessionState::Idle,
            } => RunFact::Activated(generation.number()),
            SessionEvent::StateChanged {
                state: SessionState::Unloaded,
            } => RunFact::Unloaded(generation.number()),
            _ => return Ok(GenerationPublication::Published),
        };
        self.accepted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(fact);
        self.changed.notify_waiters();
        Ok(GenerationPublication::Published)
    }
}

/// The scenario's client, agent and sink, with one live session.
async fn connected() -> (
    Arc<AgentSessionClient<NoInteraction>>,
    ScriptedLineAgent,
    Arc<RunCheckingSink>,
    String,
) {
    let (client_end, agent_end) = tokio::io::duplex(64 * 1024);
    let (agent, agent_task) = ScriptedLineAgent::start(agent_end);
    let sink = Arc::new(RunCheckingSink::default());
    let client = Arc::new(
        AgentSessionClient::initialize_in_process(
            client_end,
            agent_task,
            Arc::new(NoInteraction),
            Arc::clone(&sink) as Arc<dyn SessionEventSink>,
            Arc::new(crate::test_behavior::TestProviderBehavior),
            Arc::new(router_clock::ManualClock::new(chrono::DateTime::UNIX_EPOCH)),
        )
        .await
        .expect("the connection initializes"),
    );
    let session = client
        .create_session(PathBuf::from("/scenario"))
        .await
        .expect("session/new");
    sink.accepted.lock().expect("initial publication").clear();
    (client, agent, sink, session)
}

/// A load requested while the actor's exit is at its hand-off: its outcome
/// if it finished without reaching the agent, or None if the agent read it.
async fn load_while_held(
    client: &Arc<AgentSessionClient<NoInteraction>>,
    agent: &ScriptedLineAgent,
    session: &str,
) -> (
    Option<Result<(), ExternalProviderRuntimeError>>,
    tokio::task::JoinHandle<Result<(), ExternalProviderRuntimeError>>,
) {
    let mut loading = tokio::spawn({
        let client = Arc::clone(client);
        let session = session.to_owned();
        async move {
            client
                .load_session(session, PathBuf::from("/scenario"))
                .await
        }
    });
    let finished = tokio::select! {
        loaded = &mut loading => Some(loaded.expect("load task")),
        () = agent.received("session/load") => None,
    };
    (finished, loading)
}

/// Oracle: Lead V1 and Rev2 M4/M6, "terminal facts always delivered, in
/// order" (wrong build: the actor moves its cell and publishes Unloaded
/// after the lock, from its own task). A session's actor ends on its own;
/// while its exit is held at the hand-off a load of the same session is
/// requested. Once released, the old run's Unloaded is accepted, and it
/// precedes the new run's load.
#[test]
fn an_exited_runs_unloaded_lands_before_a_later_runs_load() {
    super::forward_named_points_to_turmoil();
    let mut simulation = turmoil::Builder::new().build();
    simulation.client("router", async move {
        // Arrange
        let mut handoff = Barrier::build(Reaction::Suspend, |point: &ClientSimPoint| {
            *point == ClientSimPoint::RunLossReady
        });
        let (client, agent, sink, session) = connected().await;

        // Act: the actor ends; a load races its hand-off.
        client
            .signal_run_loss_fixture(&session)
            .expect("the connection takes the command");
        let held = handoff.wait().await.expect("the actor's exit is held");
        let (finished, loading) = load_while_held(&client, &agent, &session).await;
        drop(held);
        let first = match finished {
            Some(outcome) => outcome,
            None => loading.await.expect("load task"),
        };
        sink.unloaded_offered().await;
        let loaded = match first {
            Ok(()) => Ok(()),
            // Refused while the old run was still live: load again.
            Err(_) => {
                client
                    .load_session(session.clone(), PathBuf::from("/scenario"))
                    .await
            }
        };
        let accepted = sink.accepted();

        // Assert
        assert!(loaded.is_ok(), "{loaded:?}");
        let unloaded = accepted
            .iter()
            .position(|fact| matches!(fact, RunFact::Unloaded(_)));
        let loading = accepted
            .iter()
            .position(|fact| matches!(fact, RunFact::Activated(_)));
        assert!(
            matches!((unloaded, loading), (Some(unloaded), Some(loading)) if unloaded < loading),
            "the old run's Unloaded precedes the new run's load: {accepted:?}"
        );
        assert!(
            matches!(
                accepted.as_slice(),
                [RunFact::Unloaded(old), RunFact::Activated(new)] if new > old
            ),
            "{accepted:?}"
        );
        Ok(())
    });
    simulation.run().expect("the scenario completes");
}

/// Oracle: Lead V1, the window case: until the connection has handled the
/// actor's exit, the session's run is still live, so a load that reaches
/// the connection first is refused as already loaded and never reaches the
/// agent; a load after the hand-off gets the old run's Unloaded and then
/// the new run's load.
#[test]
fn a_load_before_the_exit_handoff_is_refused_and_one_after_it_starts_a_new_run() {
    super::forward_named_points_to_turmoil();
    let mut simulation = turmoil::Builder::new().build();
    simulation.client("router", async move {
        // Arrange
        let mut handoff = Barrier::build(Reaction::Suspend, |point: &ClientSimPoint| {
            *point == ClientSimPoint::RunLossReady
        });
        let (client, agent, sink, session) = connected().await;

        // Act
        client
            .signal_run_loss_fixture(&session)
            .expect("the connection takes the command");
        let held = handoff.wait().await.expect("the actor's exit is held");
        let (before, loading) = load_while_held(&client, &agent, &session).await;
        let methods_while_held = agent.methods();
        drop(held);
        if before.is_none() {
            let _admitted = loading.await;
        }
        sink.unloaded_offered().await;
        let after = client
            .load_session(session.clone(), PathBuf::from("/scenario"))
            .await;
        let accepted = sink.accepted();

        // Assert
        assert!(
            matches!(
                &before,
                Some(Err(ExternalProviderRuntimeError::LocalBusy { .. }))
            ),
            "a load before the hand-off is refused as already loaded: {before:?}"
        );
        assert!(
            !methods_while_held
                .iter()
                .any(|method| method == "session/load"),
            "{methods_while_held:?}"
        );
        assert!(after.is_ok(), "{after:?}");
        assert!(
            matches!(
                accepted.as_slice(),
                [RunFact::Unloaded(old), RunFact::Activated(new)] if new > old
            ),
            "{accepted:?}"
        );
        Ok(())
    });
    simulation.run().expect("the scenario completes");
}
