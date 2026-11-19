//! A restore that a close superseded while the agent still held its load
//! answer leaves no effect on the session's later runs (review F1, F2;
//! D1b M4, "a result from an older generation changes nothing and is
//! dropped"). All in the simulation: the client, its connection and the
//! in-process agent run on one runtime; each order is decided by what the
//! agent has read and when the test releases its held answers, never by
//! time.

use super::NoInteraction;
use super::scripted_line_agent::{ModeReply, ScriptedLineAgent};
use acp_client_runtime::{
    AgentSessionClient, ExternalProviderRuntimeError, ProviderBehavior, ProviderTurnInput,
    RestoredSessionAdmission, SessionRestoreSetup,
};
use router_clock::{ClockInstant, ManualClock, RuntimeClock};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// A connected client whose one session the agent knows but the client has
/// closed, so the next step is a load.
async fn closed_session(
    clock: Arc<dyn RuntimeClock>,
    behavior: Arc<dyn ProviderBehavior>,
) -> (
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
            behavior,
            clock,
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

fn restore_clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(chrono::DateTime::UNIX_EPOCH))
}

async fn load_in_mode(
    client: &AgentSessionClient<NoInteraction>,
    session: &str,
    recorded: &str,
) -> Result<RestoredSessionAdmission, ExternalProviderRuntimeError> {
    client
        .load_session_with_setup(
            session.to_owned(),
            PathBuf::from("/scenario"),
            SessionRestoreSetup {
                requested_mode: Some(recorded.to_owned()),
                ..SessionRestoreSetup::default()
            },
        )
        .await
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
        let (client, agent, session) = closed_session(
            restore_clock(),
            Arc::new(crate::test_behavior::TestProviderBehavior),
        )
        .await;
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
        let (client, agent, session) = closed_session(
            restore_clock(),
            Arc::new(crate::test_behavior::TestProviderBehavior),
        )
        .await;
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

#[tokio::test]
async fn a_different_set_reply_keeps_the_restored_session_usable() {
    assert_restore_accepts_input_after_set(ModeReply::Selected("auto".to_owned()), Some("auto"))
        .await;
}

#[tokio::test]
async fn an_unconfirmed_set_reply_keeps_the_restored_session_usable() {
    assert_restore_accepts_input_after_set(ModeReply::NoCatalogue, None).await;
}

async fn assert_restore_accepts_input_after_set(reply: ModeReply, actual: Option<&str>) {
    // Arrange: the load initially reports plan, and the saved mode is default.
    let (client, agent, session) = closed_session(
        restore_clock(),
        Arc::new(crate::test_behavior::ClaudeLikeAutoBehavior),
    )
    .await;
    agent.restore_in_mode("plan");
    agent.next_mode_reply(reply);

    // Act: the set's own reply is the source of the post-attempt observation.
    let loaded = load_in_mode(&client, &session, "default")
        .await
        .expect("load");
    let prompt = client
        .submit_with_approval_dispatch_for_input(
            session,
            session_event_model::InputId::generate(),
            ProviderTurnInput::Prompt(session_event_model::ProviderSubmission::plain(vec![
                session_event_model::PromptContent::text("next prompt".to_owned())
                    .expect("prompt text"),
            ])),
            (),
            None,
        )
        .await;

    assert_eq!(loaded.permission_mode.actual.as_deref(), actual);
    assert!(prompt.is_ok(), "{prompt:?}");
    assert_eq!(agent.mode_sets(), ["default"]);
    assert_eq!(
        agent
            .methods()
            .iter()
            .filter(|method| *method == "session/prompt")
            .count(),
        1
    );
    client.shutdown().await;
}

#[tokio::test]
async fn a_sent_restore_mode_write_fences_a_later_load_past_the_local_deadline() {
    // Arrange: close the loading run while the agent holds its setting reply.
    let clock = restore_clock();
    let (client, agent, session) = closed_session(
        clock.clone(),
        Arc::new(crate::test_behavior::ClaudeLikeAutoBehavior),
    )
    .await;
    agent.restore_in_mode("plan");
    agent.hold_setting_answers();
    let first = spawn_load(&client, &session, Some("default"));
    agent.received("session/set_config_option").await;
    let response_wait = Duration::from_secs(30);
    clock
        .not_due_check_now(ClockInstant::ZERO.after(response_wait))
        .await;
    client
        .close_session(session.clone())
        .await
        .expect("close the pending load");

    // Act: the waiting restore times out, but its correlation stays live.
    clock.advance(response_wait).expect("reach the deadline");
    let timed_out = first.await.expect("restore task");
    let later = tokio::select! {
        () = loads_read(&agent, 2) => panic!("the later load reached ACP while the old mode write was pending"),
        result = load_in_mode(&client, &session, "default") => result,
    };

    // Assert: the later restore sends nothing until that correlation settles.
    assert!(
        matches!(
            timed_out,
            Err(ExternalProviderRuntimeError::SessionReleased)
        ),
        "{timed_out:?}"
    );
    assert!(
        matches!(later, Err(ExternalProviderRuntimeError::LocalBusy { .. })),
        "{later:?}"
    );
    assert_eq!(
        agent
            .methods()
            .iter()
            .filter(|method| *method == "session/load")
            .count(),
        1
    );
    assert_eq!(agent.mode_sets(), ["default"]);

    agent.release_setting_answers();
    agent.setting_answers_sent(1).await;
    // A request following the old response on the wire witnesses dispatch
    // through its ordered response callback, even though the run is closed.
    let _barrier = agent.request_permission(&session).await;
    let recovered = load_in_mode(&client, &session, "default")
        .await
        .expect("the old reply released the fence");
    assert_eq!(recovered.permission_mode.actual.as_deref(), Some("default"));
    assert_eq!(agent.mode_sets(), ["default", "default"]);
    client.shutdown().await;
}
