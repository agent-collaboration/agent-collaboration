//! A restore that a close superseded while the agent still held its load
//! answer leaves no effect on the session's later runs (review F1, F2;
//! D1b M4, "a result from an older generation changes nothing and is
//! dropped"). All in the simulation: the client, its connection and the
//! in-process agent run on one runtime; each order is decided by what the
//! agent has read and when the test releases its held answers, never by
//! time.

use super::NoInteraction;
use super::scripted_line_agent::ScriptedLineAgent;
use acp_client_runtime::{AgentSessionClient, ExternalProviderRuntimeError, SessionRestoreSetup};
use std::path::PathBuf;
use std::sync::Arc;

/// A connected client whose one session the agent knows but the client has
/// closed, so the next step is a load.
async fn closed_session() -> (
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
            Arc::new(super::DiscardingSink),
            Arc::new(crate::test_behavior::TestProviderBehavior),
        )
        .await
        .expect("the connection initializes"),
    );
    let session = client
        .create_session(PathBuf::from("/scenario"))
        .await
        .expect("session/new");
    client
        .close_session(session.clone())
        .await
        .expect("the session closes");
    (client, agent, session)
}

/// A load of `session` recorded in `mode`, run as its own task.
fn spawn_load(
    client: &Arc<AgentSessionClient<NoInteraction>>,
    session: &str,
    mode: Option<&str>,
) -> tokio::task::JoinHandle<Result<(), ExternalProviderRuntimeError>> {
    let client = Arc::clone(client);
    let session = session.to_owned();
    let setup = SessionRestoreSetup {
        requested_mode: mode.map(str::to_owned),
        ..SessionRestoreSetup::default()
    };
    tokio::spawn(async move {
        client
            .load_session_with_setup(session, PathBuf::from("/scenario"), setup)
            .await
            .map(|_admitted| ())
    })
}

/// Resolves once the agent has read `count` session/load requests.
async fn loads_read(agent: &ScriptedLineAgent, count: usize) {
    while agent
        .methods()
        .iter()
        .filter(|method| *method == "session/load")
        .count()
        < count
    {
        agent.received("session/load").await;
        tokio::task::yield_now().await;
    }
}

/// Oracle: review F1 and owner Decision 3 with D1b M4 (wrong build: the
/// restore writes its recorded mode without its load attempt's claim). A
/// load of a session recorded in default reaches the agent, which holds
/// its answer; the session is closed meanwhile. Released, the agent's
/// answer says the session came back in auto, but the ended attempt sends
/// no mode change, and its load ends as released.
#[test]
fn a_restore_a_close_superseded_sends_no_mode_change() {
    let mut simulation = turmoil::Builder::new().build();
    simulation.client("router", async move {
        // Arrange
        let (client, agent, session) = closed_session().await;
        agent.restore_in_mode("auto");
        agent.hold_load_answers();
        let loading = spawn_load(&client, &session, Some("default"));
        loads_read(&agent, 1).await;

        // Act: the close wins while the agent holds the load's answer.
        client
            .close_session(session.clone())
            .await
            .expect("the loading session closes");
        agent.release_load_answers();
        let loaded = loading.await.expect("load task");

        // Assert
        assert!(
            matches!(loaded, Err(ExternalProviderRuntimeError::SessionReleased)),
            "the superseded load ends as released: {loaded:?}"
        );
        assert_eq!(
            agent.mode_sets(),
            Vec::<String>::new(),
            "the ended load attempt changed the session's mode at the agent"
        );
        Ok(())
    });
    simulation.run().expect("the scenario completes");
}

/// Oracle: review F2 and the run-ownership contract (wrong build: the
/// stale completion removes the session from request routing). Load A
/// reaches the agent, which holds its answer; a close ends it and load B
/// starts. A's late completion is dropped, B loads, and a permission
/// request the agent sends for the session afterwards still reaches the
/// client's handler instead of being refused as an unknown session.
#[test]
fn a_superseded_restores_late_completion_leaves_the_newer_runs_routing() {
    let mut simulation = turmoil::Builder::new().build();
    simulation.client("router", async move {
        // Arrange
        let (client, agent, session) = closed_session().await;
        agent.hold_load_answers();
        let first = spawn_load(&client, &session, None);
        loads_read(&agent, 1).await;
        client
            .close_session(session.clone())
            .await
            .expect("the loading session closes");
        let second = spawn_load(&client, &session, None);
        loads_read(&agent, 2).await;

        // Act
        agent.release_load_answers();
        let first_loaded = first.await.expect("first load task");
        let second_loaded = second.await.expect("second load task");
        let response = agent.request_permission(&session).await;

        // Assert
        assert!(
            matches!(
                first_loaded,
                Err(ExternalProviderRuntimeError::SessionReleased)
            ),
            "{first_loaded:?}"
        );
        assert!(second_loaded.is_ok(), "{second_loaded:?}");
        assert_ne!(
            response
                .pointer("/error/code")
                .and_then(serde_json::Value::as_i64),
            Some(-32601),
            "the newer run's request was refused as an unknown session: {response}"
        );
        Ok(())
    });
    simulation.run().expect("the scenario completes");
}
