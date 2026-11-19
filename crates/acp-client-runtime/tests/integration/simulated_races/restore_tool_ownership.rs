//! Wire oracles for retired tool ownership during restore and before live input.

use super::{
    AgentSessionClient, ClockInstant, Duration, ExternalProviderRuntimeError, NoInteraction,
    RestoreObservations, ScriptedLineAgent, closed_session_with_sink, prompt_restored_session,
    restore_clock, restore_without_mode, spawn_load,
};
use session_event_model::{SessionEvent, SessionItem};
use std::{future::Future, sync::Arc};

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
    let first = restore_without_mode(&client, &session, "session/load");
    tokio::pin!(first);
    let deadline = Duration::from_secs(30);
    tokio::select! {
        () = clock.not_due_check_now(ClockInstant::ZERO.after(deadline)) => {},
        result = &mut first => panic!("load A ended before its deadline: {result:?}"),
    }
    agent.received("session/load").await;
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
