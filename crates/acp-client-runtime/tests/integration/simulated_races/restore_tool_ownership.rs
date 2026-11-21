//! Wire oracles for retired tool ownership during restore and before live input.

use super::{
    AgentSessionClient, Duration, ExternalProviderRuntimeError, NoInteraction, RestoreObservations,
    ScriptedLineAgent, closed_session_with_sink, prompt_restored_session, restore_clock,
    restore_without_mode, spawn_load,
};
use acp_client_runtime::{
    ApprovalPortOutcome, InteractionFuture, InteractionPort, RefusedApprovalOffer,
};
use router_clock::RuntimeClock as _;
use serde_json::json;
use session_event_model::{SessionEvent, SessionItem};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{future::Future, sync::Arc};
use tokio_util::sync::CancellationToken;

fn items_for_tool(observations: &RestoreObservations, tool_id: &str) -> Vec<SessionItem> {
    observations
        .events
        .lock()
        .expect("session observations")
        .iter()
        .filter_map(|event| match event {
            SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item }
                if item.item_id == tool_id =>
            {
                Some(item.clone())
            }
            _ => None,
        })
        .collect()
}

#[derive(Default)]
struct CountApprovals(AtomicUsize);

impl InteractionPort for CountApprovals {
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
        _turn: CancellationToken,
        _agent: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { ApprovalPortOutcome::Cancelled })
    }

    fn request_question(
        &self,
        _context: (),
        _request: session_event_model::QuestionRequest,
        _turn: CancellationToken,
        _agent: CancellationToken,
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

#[tokio::test]
async fn a_shared_tool_callback_stays_ambiguous_after_one_session_retires() {
    let observations = Arc::new(RestoreObservations::default());
    let approvals = Arc::new(CountApprovals::default());
    let (client_end, agent_end) = tokio::io::duplex(64 * 1024);
    let (agent, agent_task) = ScriptedLineAgent::start(agent_end);
    let client = AgentSessionClient::initialize_in_process(
        client_end,
        agent_task,
        approvals.clone(),
        observations.clone(),
        Arc::new(crate::test_behavior::TestProviderBehavior),
        restore_clock(),
    )
    .await
    .expect("initialize ACP");
    let first = client
        .create_session("/scenario".into())
        .await
        .expect("session A");
    let second = client
        .create_session("/scenario".into())
        .await
        .expect("session B");
    let shared = "same-tool-id";
    agent.announce_replay_tool(&first, shared).await;
    tool_item_observed(&observations, shared, "Provisional tool").await;

    agent.hold_next_prompt();
    let input = client.submit_with_approval_dispatch_for_input(
        second.clone(),
        session_event_model::InputId::generate(),
        acp_client_runtime::ProviderTurnInput::Prompt(
            session_event_model::ProviderSubmission::plain(vec![
                session_event_model::PromptContent::text("hold the second session open".to_owned())
                    .expect("prompt text"),
            ]),
        ),
        (),
        None,
    );
    tokio::pin!(input);
    tokio::select! {
        () = agent.wait_for_held_prompt() => {},
        result = &mut input => panic!("B ended before the callback barrier: {result:?}"),
    }
    agent.announce_replay_tool(&second, shared).await;
    // Both frames travel through B's ordered ACP stream. Receiving the
    // independent tool proves B projected its shared tool first.
    agent.announce_replay_tool(&second, "unique-control").await;
    tool_item_observed(&observations, "unique-control", "Provisional tool").await;
    assert_eq!(items_for_tool(&observations, shared).len(), 2);
    let control = agent
        .request_callback(
            "unique-plan",
            "cursor/create_plan",
            json!({
                "toolCallId": "unique-control", "name": "unique plan", "plan": "control"
            }),
        )
        .await;
    assert_eq!(control.get("result"), Some(&json!({"outcome":"cancelled"})));
    assert_eq!(approvals.0.load(Ordering::SeqCst), 1);

    client
        .close_session(first)
        .await
        .expect("retire A while B is running");
    let late_plan = agent
        .request_callback(
            "shared-plan",
            "cursor/create_plan",
            json!({
                "toolCallId": shared, "name": "ambiguous callback plan", "plan": "do not publish"
            }),
        )
        .await;
    let late_todo = agent.request_callback("shared-todo", "cursor/update_todos", json!({
        "toolCallId": shared, "todos": [{"id":"stale","content":"ambiguous callback todo","status":"pending"}],
        "merge": false
    })).await;
    agent.release_held_prompt().await;
    input.await.expect("B completes its original input");

    assert_eq!(
        late_plan.get("result"),
        Some(&json!({"outcome":"cancelled"}))
    );
    assert_eq!(late_todo.get("result"), Some(&json!({})));
    assert_eq!(
        approvals.0.load(Ordering::SeqCst),
        1,
        "the ambiguous callback used B's interaction"
    );
    assert!(
        items_for_tool(&observations, &format!("plan:{shared}")).is_empty(),
        "the ambiguous callback published in B"
    );
    assert!(
        !observations.events.lock().expect("session observations").iter().any(|event| matches!(event,
            SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item }
                if item.text.as_deref().is_some_and(|text| text.contains("ambiguous callback todo"))
        )),
        "the ambiguous todo callback published in B"
    );
    assert_eq!(agent.prompt_sessions(), [second]);
    client.shutdown().await;
}

async fn tool_item_observed(observations: &RestoreObservations, tool_id: &str, title: &str) {
    loop {
        let changed = observations.events_changed.notified();
        if items_for_tool(observations, tool_id)
            .iter()
            .any(|item| item.text.as_deref() == Some(title))
        {
            return;
        }
        changed.await;
    }
}

async fn during_prompt(
    client: &AgentSessionClient<NoInteraction>,
    agent: &ScriptedLineAgent,
    session: &str,
    work: impl Future<Output = ()>,
) {
    agent.hold_next_prompt();
    let input = prompt_restored_session(client, session);
    tokio::pin!(input);
    tokio::select! {
        () = agent.wait_for_held_prompt() => {},
        () = &mut input => panic!("the prompt ended before the wire barrier"),
    }
    work.await;
    agent.release_held_prompt().await;
    input.await;
}

#[tokio::test]
async fn an_idle_resume_drops_the_expired_restores_tool_frames_before_input() {
    let clock = restore_clock();
    let observations = Arc::new(RestoreObservations::default());
    let (client, agent, session) = closed_session_with_sink(
        clock.clone(),
        Arc::new(crate::test_behavior::TestProviderBehavior),
        observations.clone(),
    )
    .await;
    agent.hold_next_restore_answer();
    clock
        .advance(Duration::from_millis(1))
        .expect("separate restore from the previous close deadline");
    let first = restore_without_mode(&client, &session, "session/load");
    tokio::pin!(first);
    let deadline = Duration::from_secs(30);
    let due = clock.monotonic_now().after(deadline);
    tokio::select! {
        () = async {
            agent.received("session/load").await;
            clock.not_due_check_now(due).await;
        } => {},
        result = &mut first => panic!("load A ended before its deadline: {result:?}"),
    }
    agent.announce_replay_tool(&session, "expired-tool").await;
    // That callback's response establishes that the connection read the announcement.
    let _response = agent.request_permission(&session).await;
    clock.advance(deadline).expect("expire load A");
    assert!(matches!(
        first.await,
        Err(ExternalProviderRuntimeError::RestoreNoAnswer)
    ));

    restore_without_mode(&client, &session, "session/resume")
        .await
        .expect("resume B on the same connection");
    agent
        .update_tool(&session, "expired-tool", "late idle update")
        .await;
    agent.announce_replay_tool(&session, "expired-tool").await;
    agent.announce_replay_tool(&session, "idle-control").await;
    tool_item_observed(&observations, "idle-control", "Provisional tool").await;

    assert!(
        agent.prompt_sessions().is_empty(),
        "B has not received input yet"
    );
    let stale = items_for_tool(&observations, "expired-tool");
    assert!(
        stale.is_empty(),
        "B projected A's retired tool while idle: {stale:?}"
    );
    client.shutdown().await;
}

#[tokio::test]
async fn replay_after_a_resume_refreshes_the_tool_owner_through_its_next_turn() {
    let clock = restore_clock();
    let observations = Arc::new(RestoreObservations::default());
    let (client, agent, session) = closed_session_with_sink(
        clock,
        Arc::new(crate::test_behavior::TestProviderBehavior),
        observations.clone(),
    )
    .await;
    let tool_id = "replayed-across-runs";
    restore_without_mode(&client, &session, "session/resume")
        .await
        .expect("run A");
    during_prompt(&client, &agent, &session, async {
        agent.announce_replay_tool(&session, tool_id).await;
        tool_item_observed(&observations, tool_id, "Provisional tool").await;
    })
    .await;
    client
        .close_session(session.clone())
        .await
        .expect("close A");
    restore_without_mode(&client, &session, "session/resume")
        .await
        .expect("run B resumes without replay");
    client
        .close_session(session.clone())
        .await
        .expect("close B");

    agent.hold_next_restore_answer();
    let replay = spawn_load(&client, &session, None);
    agent.received("session/load").await;
    agent.announce_replay_tool(&session, tool_id).await;
    agent.release_load_answers();
    replay
        .await
        .expect("load task")
        .expect("run C loads its history");
    assert_eq!(
        items_for_tool(&observations, tool_id).len(),
        2,
        "A and C both observed the tool"
    );

    during_prompt(&client, &agent, &session, async {
        agent
            .update_tool(&session, tool_id, "C's first turn continues replay")
            .await;
        tool_item_observed(&observations, tool_id, "C's first turn continues replay").await;
    })
    .await;
    let before = items_for_tool(&observations, tool_id).len();
    during_prompt(&client, &agent, &session, async {
        agent
            .update_tool(&session, tool_id, "expired replay continuation")
            .await;
        agent.send_stale_tool_callbacks(tool_id).await;
    })
    .await;

    let items = items_for_tool(&observations, tool_id);
    assert_eq!(
        items.len(),
        before,
        "C's second turn received a retired tool update: {items:?}"
    );
    assert!(
        !observations.events.lock().expect("session observations").iter().any(|event| matches!(event,
            SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item }
                if item.text.as_deref().is_some_and(|text| text.contains("expired restore todo sentinel") || text.contains("expired restore plan sentinel"))
        )),
        "C's second turn received a retired tool callback"
    );
    assert_eq!(agent.prompt_sessions(), [session.as_str(); 3]);
    client.shutdown().await;
}
