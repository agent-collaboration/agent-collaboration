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

#[path = "restore_tool_ownership.rs"]
mod tool_ownership;

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
    closed_session_with_sink(clock, behavior, Arc::new(super::DiscardingSink)).await
}

async fn closed_session_with_sink(
    clock: Arc<dyn RuntimeClock>,
    behavior: Arc<dyn ProviderBehavior>,
    sink: Arc<dyn acp_client_runtime::SessionEventSink>,
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
            sink,
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

/// A silent optional setter stops delaying activation at the injected
/// deadline, and excludes only overlapping restore setters.
#[tokio::test]
async fn a_silent_restore_setter_allows_input_and_a_same_session_reload() {
    let clock = restore_clock();
    let (client, agent, session) = closed_session(
        clock.clone(),
        Arc::new(crate::test_behavior::ClaudeLikeAutoBehavior),
    )
    .await;
    agent.restore_in_mode("plan");
    agent.hold_setting_answers();
    let first_restore = load_in_mode(&client, &session, "default");
    let mut first_restore = Box::pin(first_restore);
    let response_wait = Duration::from_secs(30);
    tokio::select! {
        () = clock.not_due_check_now(ClockInstant::ZERO.after(response_wait)) => {},
        result = &mut first_restore => panic!("the optional setter had not settled: {result:?}"),
    }
    agent.received("session/set_config_option").await;
    clock.advance(response_wait).expect("reach the deadline");
    let first = first_restore
        .await
        .expect("a silent setter does not make load fail");
    assert_eq!(first.permission_mode.actual, None);
    assert_eq!(
        first.mode_difference,
        Some(acp_client_runtime::RestoredModeDifference::NoAnswer)
    );
    prompt_restored_session(&client, &session).await;
    client
        .close_session(session.clone())
        .await
        .expect("close the active first run");

    let second_restore = load_in_mode(&client, &session, "default");
    let mut second_restore = Box::pin(second_restore);
    let second = tokio::select! {
        result = &mut second_restore => result.expect("the earlier setter cannot hold reload"),
        () = agent.mode_sets_received(2) => panic!("the next restore sent an overlapping setter"),
    };
    assert!(second.generation.number() > first.generation.number());
    assert_eq!(second.permission_mode.actual, None);
    assert_eq!(
        second.mode_difference,
        Some(acp_client_runtime::RestoredModeDifference::EarlierWriteUnanswered)
    );
    prompt_restored_session(&client, &session).await;
    assert_eq!(
        agent.prompt_sessions(),
        [session.as_str(), session.as_str()]
    );

    agent.release_setting_answers();
    agent.setting_answers_sent(1).await;
    let _barrier = agent.request_permission(&session).await;
    assert_eq!(
        client
            .settings_catalog(&session)
            .expect("live catalog")
            .effective_settings()
            .mode,
        None
    );
    assert_eq!(agent.mode_sets(), ["default"]);
    assert_eq!(
        agent.prompt_sessions(),
        [session.as_str(), session.as_str()]
    );
    client
        .close_session(session.clone())
        .await
        .expect("close the second run");
    let recovered = load_in_mode(&client, &session, "default")
        .await
        .expect("after the late reply");
    assert_eq!(recovered.permission_mode.actual.as_deref(), Some("default"));
    assert_eq!(agent.mode_sets(), ["default", "default"]);
    client.shutdown().await;
}

async fn prompt_restored_session(client: &AgentSessionClient<NoInteraction>, session: &str) {
    let prompt = ProviderTurnInput::Prompt(session_event_model::ProviderSubmission::plain(vec![
        session_event_model::PromptContent::text("continue once".to_owned()).expect("prompt text"),
    ]));
    client
        .submit_with_approval_dispatch_for_input(
            session.to_owned(),
            session_event_model::InputId::generate(),
            prompt,
            (),
            None,
        )
        .await
        .expect("restored input completes");
}

#[derive(Default)]
struct RestoreObservations {
    events: std::sync::Mutex<Vec<session_event_model::SessionEvent>>,
    events_changed: tokio::sync::Notify,
    retired_entries: std::sync::Mutex<Vec<(String, String)>>,
}

impl acp_client_runtime::SessionEventSink for RestoreObservations {
    fn publish_skill_view(
        &self,
        _id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), acp_client_runtime::EventSinkClosed> {
        Ok(())
    }

    fn begin_history_replay(&self, _id: &str) -> acp_client_runtime::HistoryReplayFuture<'_> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn publish(
        &self,
        _id: &str,
        event: session_event_model::SessionEvent,
    ) -> Result<(), acp_client_runtime::EventSinkClosed> {
        self.events
            .lock()
            .map_err(|_| acp_client_runtime::EventSinkClosed)?
            .push(event);
        self.events_changed.notify_waiters();
        Ok(())
    }
}

impl acp_client_runtime::QueueEntrySink for RestoreObservations {
    fn entry_not_sent(&self, _session: &str, operation: &str, reason: &'static str) {
        self.retired_entries
            .lock()
            .expect("queued entries")
            .push((operation.to_owned(), reason.to_owned()));
    }
}

#[tokio::test]
async fn an_unanswered_load_retires_its_queue_and_allows_a_same_session_retry() {
    for method in ["session/load", "session/resume"] {
        let clock = restore_clock();
        let observations = Arc::new(RestoreObservations::default());
        let (client, agent, session) = closed_session_with_sink(
            clock.clone(),
            Arc::new(crate::test_behavior::TestProviderBehavior),
            observations.clone(),
        )
        .await;
        client.set_queue_entry_sink(observations.clone());
        let independent = client
            .create_session(PathBuf::from("/scenario"))
            .await
            .expect("independent session");
        agent.hold_next_restore_answer();
        let mut first = Box::pin(restore_without_mode(&client, &session, method));
        let delay = Duration::from_secs(30);
        tokio::select! {
            () = clock.not_due_check_now(ClockInstant::ZERO.after(delay)) => {},
            result = &mut first => panic!("the unanswered restore ended before the deadline: {result:?}"),
        }
        agent.received(method).await;
        agent
            .send_replay_chunk(&session, "expired restore replay sentinel")
            .await;
        agent.announce_replay_tool(&session, "expired-tool").await;
        let queued_id = uuid::Uuid::now_v7().to_string();
        let queued = client
            .queue_enqueue(&session, &queued_id)
            .expect("input waits in A's line");
        prompt_restored_session(&client, &independent).await;
        assert_eq!(agent.prompt_sessions(), [independent.as_str()]);
        clock.advance(delay).expect("reach the restore deadline");
        let failure = first.await.expect_err("A never answered");
        assert!(
            matches!(failure, ExternalProviderRuntimeError::RestoreNoAnswer),
            "{failure:?}"
        );
        assert!(failure.to_string().contains("retry this session"));
        assert_eq!(
            observations
                .retired_entries
                .lock()
                .expect("queue receipts")
                .len(),
            1
        );
        assert_eq!(
            observations.retired_entries.lock().expect("queue receipts")[0].0,
            queued_id
        );

        let later = restore_without_mode(&client, &session, method)
            .await
            .expect("deliberate same-ID retry succeeds while A is silent");
        assert!(later.earlier_restore_unanswered);
        assert!(later.generation.number() > queued.generation.number());
        agent.hold_next_prompt();
        let retry_input = prompt_restored_session(&client, &session);
        tokio::pin!(retry_input);
        tokio::select! {
            () = agent.wait_for_held_prompt() => {},
            () = &mut retry_input => panic!("the retry input ended before the callback barrier"),
        }
        agent.send_stale_tool_callbacks("expired-tool").await;
        agent.release_held_prompt().await;
        retry_input.await;
        agent.release_load_answers();
        agent.held_load_answer_sent().await;
        let barrier = agent.request_permission(&session).await;
        assert_ne!(
            barrier
                .pointer("/error/code")
                .and_then(serde_json::Value::as_i64),
            Some(-32601)
        );
        assert_eq!(
            agent.prompt_sessions(),
            [independent.as_str(), session.as_str()]
        );
        assert!(!observations.events.lock().expect("events").iter().any(|event| matches!(event,
            session_event_model::SessionEvent::ItemStarted{ item } | session_event_model::SessionEvent::ItemUpdated{ item }
                if item.text.as_deref().is_some_and(|text| text.contains("expired restore todo sentinel") || text.contains("expired restore plan sentinel"))
        )), "the retry received a callback belonging to the expired load");
        assert!(!observations.events.lock().expect("events").iter().any(|event| matches!(event,
            session_event_model::SessionEvent::ItemStarted{ item } if item.text.as_deref() == Some("expired restore replay sentinel")
        )));
        client.shutdown().await;
    }
}

#[tokio::test]
async fn a_lost_session_nudges_a_new_session_without_starting_one() {
    for (code, sentence) in [
        (-32002, "start a new session with the same setup"),
        (-32041, "the provider refused to restore this session"),
        (
            -32603,
            "the provider failed while restoring this session, so retry it",
        ),
    ] {
        let (client, agent, session) = closed_session(
            restore_clock(),
            Arc::new(crate::test_behavior::TestProviderBehavior),
        )
        .await;
        agent.reject_next_restore(code);
        let failure = client
            .resume_session_with_setup(
                session.clone(),
                PathBuf::from("/scenario"),
                SessionRestoreSetup::default(),
            )
            .await
            .expect_err("the harness refused restore");
        assert!(failure.to_string().contains(sentence), "{failure:?}");
        let seen = agent.methods();
        assert_eq!(
            seen.iter()
                .filter(|method| *method == "session/new")
                .count(),
            1
        );
        assert!(agent.prompt_sessions().is_empty());
        let accepted = client
            .resume_session_with_setup(
                session.clone(),
                PathBuf::from("/scenario"),
                SessionRestoreSetup::default(),
            )
            .await
            .expect("a new deliberate attempt may succeed");
        assert!(!accepted.earlier_restore_unanswered);
        prompt_restored_session(&client, &session).await;
        assert_eq!(agent.prompt_sessions(), [session.as_str()]);
        client.shutdown().await;
    }
}

async fn restore_without_mode(
    client: &AgentSessionClient<NoInteraction>,
    session: &str,
    method: &str,
) -> Result<RestoredSessionAdmission, ExternalProviderRuntimeError> {
    let id = session.to_owned();
    let cwd = PathBuf::from("/scenario");
    let setup = SessionRestoreSetup::default();
    if method == "session/resume" {
        client.resume_session_with_setup(id, cwd, setup).await
    } else {
        client.load_session_with_setup(id, cwd, setup).await
    }
}
