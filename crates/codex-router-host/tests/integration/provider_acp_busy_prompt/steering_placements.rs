//! A busy `session/prompt` to an agent that steers (the send-race fixture),
//! through the real Host command port: steered into the running turn, or, when
//! the agent says the turn ended, re-decided once, prompted or queued.

use super::acp_busy_host::{AcpBusyHost, BusyAgent, HubTurns, agent_frames, until_agent_read};
use session_event_model::{SessionEvent, StopReason};

/// The message whose turn is running.
const FIRST: &str = "first";
/// The message sent while it runs: the busy prompt under test.
const SECOND: &str = "second";

/// Oracle: D1 R12, V22b — "Steered: the command port records one steer, and
/// the response comes when that running turn ends. No Busy. Zero cancels."
/// The agent receives one steer carrying the message and no prompt for it,
/// the hub records the input accepted into the running turn and starts no
/// other turn, the prompt is unanswered while that turn runs, and its
/// response carries that turn's stop reason (the fixture ends it max_tokens).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn steered_busy_prompt_answers_when_the_running_turn_ends() {
    // Arrange: a steering agent whose first turn is held, ending max_tokens.
    let mut host = AcpBusyHost::start(&BusyAgent::SendRace(&["first-turn-ends-max-tokens"])).await;
    let mut events = host.attach_events().await.receiver;
    host.send_prompt(3, FIRST).await;
    until_agent_read(&host.receipt, "session/prompt", 1).await;
    let foreground_turn = loop {
        let event = events.recv().await.expect("foreground session events");
        match event.event {
            SessionEvent::TurnStarted { turn_id, .. } => break turn_id,
            SessionEvent::ResyncRequired { .. } => panic!("hub resynced before foreground start"),
            _ => {}
        }
    };

    // Act: the busy prompt; once its steer reached the agent, look again, then end the turn.
    host.send_prompt(4, SECOND).await;
    until_agent_read(&host.receipt, "_session/steering", 1).await;
    loop {
        let event = events.recv().await.expect("acceptance session events");
        match event.event {
            SessionEvent::InputAccepted { turn_id, .. } if turn_id == foreground_turn => break,
            SessionEvent::TurnEnded { .. } | SessionEvent::ResyncRequired { .. } => {
                panic!(
                    "the held turn must accept the steer before ending: {:?}",
                    event.event
                )
            }
            _ => {}
        }
    }
    let line_after_steer = host.queue_list().await;
    let answered_while_running = host.has_answered(4);
    host.release("release");
    let first = host.response(3).await;
    let second = host.response(4).await;
    let events = host.hub_events().await;
    let turns = HubTurns::from_observed(&events);
    let steers = agent_frames(&host.receipt, "_session/steering", Some(SECOND));
    let second_prompts = agent_frames(&host.receipt, "session/prompt", Some(SECOND));
    let cancels = agent_frames(&host.receipt, "session/cancel", None);
    let stopped = host.shutdown().await;

    // Assert: one steer and nothing else for the message.
    assert_eq!(steers, 1, "one steer");
    assert_eq!(second_prompts, 0, "a steered message is not also prompted");
    assert!(
        line_after_steer.is_empty(),
        "nothing queued: {line_after_steer:?}"
    );
    // Assert: it joined the running turn and answered at that turn's end.
    assert!(
        !answered_while_running,
        "unanswered while the turn runs: {second}"
    );
    assert_eq!(
        turns.started.len(),
        1,
        "no other turn starts: {turns:?}; observed {events:?}"
    );
    let running_turn = turns.started[0].0.as_str();
    assert!(
        turns.accepted.iter().any(|(_, turn)| turn == running_turn),
        "the running turn accepted the steer: {turns:?}"
    );
    assert_eq!(
        turns.stop_reason_of(running_turn),
        Some(&StopReason::MaxTokens),
        "{turns:?}"
    );
    assert_eq!(first["result"]["stopReason"], "max_tokens", "{first}");
    assert_eq!(
        second["result"]["stopReason"], "max_tokens",
        "answered at the running turn's end: {second}"
    );
    assert_eq!(cancels, 0, "no cancel");
    stopped.expect("Host shutdown");
}

/// Oracle: D1 R12, V22b — "Just-ended: one prompt. No Busy. Zero cancels."
/// The agent ends its running turn as the steer arrives and answers the steer
/// promptRequired, so nothing was injected; the message goes to the agent as
/// exactly one prompt, and its response is a normal turn end, never an error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn busy_prompt_whose_turn_just_ended_is_prompted_once() {
    // Arrange: a steering agent that ends its running turn when the first steer arrives.
    let mut host = AcpBusyHost::start(&BusyAgent::SendRace(&["first-steer-ends-turn"])).await;
    host.send_prompt(3, FIRST).await;
    until_agent_read(&host.receipt, "session/prompt", 1).await;

    // Act
    host.send_prompt(4, SECOND).await;
    let first = host.response(3).await;
    let second = host.response(4).await;
    let steers = agent_frames(&host.receipt, "_session/steering", Some(SECOND));
    let second_prompts = agent_frames(&host.receipt, "session/prompt", Some(SECOND));
    let cancels = agent_frames(&host.receipt, "session/cancel", None);
    let stopped = host.shutdown().await;

    // Assert
    assert_eq!(first["result"]["stopReason"], "end_turn", "{first}");
    assert_eq!(steers, 1, "one steer, answered promptRequired");
    assert_eq!(second_prompts, 1, "one prompt");
    assert_eq!(
        second["result"]["stopReason"], "end_turn",
        "never busy: {second}"
    );
    assert_eq!(cancels, 0, "no cancel");
    stopped.expect("Host shutdown");
}

/// Oracle: D1 R12, V22b — PromptRequired contention: "Races are bounded to
/// one retry each; a second contention queues." No Busy, zero cancels. The
/// agent answers the steer promptRequired while its turn still runs, so the
/// one re-decision's prompt meets the running turn again; the message then
/// waits as one input in the line, is never steered or prompted again while
/// the turn runs, is sent once after it ends, and answers at the end of the
/// turn its own input ID started (end_turn, not the running turn's max_tokens).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prompt_required_while_the_turn_still_runs_queues_once() {
    // Arrange: a steering agent that answers the first steer promptRequired mid-turn.
    let mut host = AcpBusyHost::start(&BusyAgent::SendRace(&[
        "first-steer-before-turn-end",
        "first-turn-ends-max-tokens",
    ]))
    .await;
    host.send_prompt(3, FIRST).await;
    until_agent_read(&host.receipt, "session/prompt", 1).await;

    // Act: the busy prompt; read the line while it waits; release the turn.
    host.send_prompt(4, SECOND).await;
    let waiting = host.until_queued_or_answered(1, 4).await;
    let answered_while_waiting = host.has_answered(4);
    let steers_while_waiting = agent_frames(&host.receipt, "_session/steering", Some(SECOND));
    let prompts_while_waiting = agent_frames(&host.receipt, "session/prompt", Some(SECOND));
    host.release("release");
    let first = host.response(3).await;
    let second = host.response(4).await;
    let turns = HubTurns::from_observed(&host.hub_events().await);
    let steers = agent_frames(&host.receipt, "_session/steering", Some(SECOND));
    let second_prompts = agent_frames(&host.receipt, "session/prompt", Some(SECOND));
    let cancels = agent_frames(&host.receipt, "session/cancel", None);
    let stopped = host.shutdown().await;

    // Assert: one steer, then the line, with nothing sent while the turn runs.
    assert!(!answered_while_waiting, "never answered busy: {second}");
    assert_eq!(
        waiting.len(),
        1,
        "the second contention queues: {waiting:?}"
    );
    assert_eq!(steers_while_waiting, 1, "one steer, no second re-decision");
    assert_eq!(prompts_while_waiting, 0, "nothing prompted while it waits");
    // Assert: sent once, under the queued ID, answered at its own turn's end.
    let input_id = waiting[0]["inputId"].as_str().expect("queued input ID");
    let own_turn = turns.started_by(input_id);
    assert!(
        own_turn.is_some(),
        "the queued input ID starts a turn: {turns:?}"
    );
    assert_eq!(
        own_turn.and_then(|turn| turns.stop_reason_of(turn)),
        Some(&StopReason::EndTurn),
        "{turns:?}"
    );
    assert_eq!(first["result"]["stopReason"], "max_tokens", "{first}");
    assert_eq!(second["result"]["stopReason"], "end_turn", "{second}");
    assert_eq!((steers, second_prompts), (1, 1), "one steer, one prompt");
    assert_eq!(cancels, 0, "no cancel");
    stopped.expect("Host shutdown");
}
