//! An actor that ends on its own hands its run's end to the session's cell
//! and the hub before any later run of the session can load (D1 R14; Rev2
//! M4 and M6, terminal facts delivered in order; Lead V1). The sink here
//! checks each claim against the client's session runs, as the Host's does.

use super::NoInteraction;
use super::scripted_line_agent::ScriptedLineAgent;
use acp_client_runtime::sim_points::ClientSimPoint;
use acp_client_runtime::{
    AgentSessionClient, EventSinkClosed, ExternalProviderRuntimeError, GenerationPublication,
    HistoryReplayFuture, SessionEventSink, SessionRuns,
};
use provider_session_machines::SessionGeneration;
use provider_session_machines::session_run_claims::RunClaim;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use turmoil::barriers::{Barrier, Reaction};

/// A run fact the hub accepted, with its run's number.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RunFact {
    Unloaded(u64),
    Loading(u64),
}

/// Records the run facts whose claims the session's runs admit.
#[derive(Default)]
struct RunCheckingSink {
    runs: Mutex<Option<Arc<dyn SessionRuns>>>,
    accepted: Mutex<Vec<RunFact>>,
    unloaded_outcome: Mutex<Option<GenerationPublication>>,
    unloaded_attempted: tokio::sync::Notify,
}

impl RunCheckingSink {
    fn admit(&self, session_id: &str, claim: RunClaim, fact: RunFact) -> GenerationPublication {
        let runs = self
            .runs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let Some(runs) = runs else {
            return GenerationPublication::Ended;
        };
        runs.publish_if(session_id, claim, &mut || {
            self.accepted
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(fact);
            Ok(())
        })
        .unwrap_or(GenerationPublication::Ended)
    }

    fn accepted(&self) -> Vec<RunFact> {
        self.accepted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn unloaded_outcome(&self) -> Option<GenerationPublication> {
        *self
            .unloaded_outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Resolves once an Unloaded has been offered, whatever its outcome.
    async fn unloaded_offered(&self) {
        loop {
            let offered = self.unloaded_attempted.notified();
            if self.unloaded_outcome().is_some() {
                return;
            }
            offered.await;
        }
    }
}

impl SessionEventSink for RunCheckingSink {
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

    fn attach_session_runs(&self, runs: Arc<dyn SessionRuns>) {
        *self.runs.lock().unwrap_or_else(PoisonError::into_inner) = Some(runs);
    }

    fn begin_history_replay_in(
        &self,
        session_id: &str,
        generation: SessionGeneration,
    ) -> HistoryReplayFuture<'_> {
        let _accepted = self.admit(
            session_id,
            RunClaim::Exact(generation),
            RunFact::Loading(generation.number()),
        );
        Box::pin(async { Ok(()) })
    }

    fn publish_unloaded(
        &self,
        session_id: &str,
        generation: SessionGeneration,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        let outcome = self.admit(
            session_id,
            RunClaim::Terminal(generation),
            RunFact::Unloaded(generation.number()),
        );
        *self
            .unloaded_outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(outcome);
        self.unloaded_attempted.notify_waiters();
        Ok(outcome)
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
        )
        .await
        .expect("the connection initializes"),
    );
    let session = client
        .create_session(PathBuf::from("/scenario"))
        .await
        .expect("session/new");
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
            *point == ClientSimPoint::ActorExitHandoff
        });
        let (client, agent, sink, session) = connected().await;

        // Act: the actor ends; a load races its hand-off.
        client
            .end_session_actor(session.clone())
            .await
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
        assert_eq!(
            sink.unloaded_outcome(),
            Some(GenerationPublication::Published),
            "the exited run's Unloaded was refused: {accepted:?}"
        );
        let unloaded = accepted
            .iter()
            .position(|fact| matches!(fact, RunFact::Unloaded(_)));
        let loading = accepted
            .iter()
            .position(|fact| matches!(fact, RunFact::Loading(_)));
        assert!(
            matches!((unloaded, loading), (Some(unloaded), Some(loading)) if unloaded < loading),
            "the old run's Unloaded precedes the new run's load: {accepted:?}"
        );
        assert!(
            matches!(
                accepted.as_slice(),
                [RunFact::Unloaded(old), RunFact::Loading(new)] if new > old
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
            *point == ClientSimPoint::ActorExitHandoff
        });
        let (client, agent, sink, session) = connected().await;

        // Act
        client
            .end_session_actor(session.clone())
            .await
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
                [RunFact::Unloaded(old), RunFact::Loading(new)] if new > old
            ),
            "{accepted:?}"
        );
        Ok(())
    });
    simulation.run().expect("the scenario completes");
}
