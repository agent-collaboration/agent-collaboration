//! A busy `session/prompt` to an agent that cannot steer (the Cursor
//! fixture), through the real Host command port: it waits in the session's
//! Router line, and answers at the end of the turn its own input started, or
//! with an error naming why it was never sent.

use super::acp_busy_host::{AcpBusyHost, BusyAgent, HubTurns, agent_frames, until_agent_read};
use session_event_model::StopReason;

/// The message whose turn is running.
const FIRST: &str = "first";
/// The message sent while it runs: the busy prompt under test.
const SECOND: &str = "second";

/// Oracle: D1 R12, V22b — "Queued: one queue_add, and the response comes when
/// the turn that starts with the input ID it returned ends. No Busy. Zero
/// cancels." The busy prompt waits as exactly one input in the line, the agent
/// sees no second prompt while the first turn runs, the turn the hub started
/// with that input's ID is a new turn, and the prompt's response carries that
/// turn's stop reason, not the running turn's (the fixture ends the running
/// turn with max_tokens so the two ends differ).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_busy_prompt_answers_when_the_turn_its_input_started_ends() {
    // Arrange: a non-steering agent whose first turn is held, ending max_tokens.
    let mut host = AcpBusyHost::start(&BusyAgent::Cursor(&[
        "hold-first-prompt-open",
        "held-turn-ends-max-tokens",
    ]))
    .await;
    host.send_prompt(3, FIRST).await;
    until_agent_read(&host.receipt, "session/prompt", 1).await;

    // Act: the busy prompt; read the line while it waits; release the turn.
    host.send_prompt(4, SECOND).await;
    let waiting = host.until_queued_or_answered(1, 4).await;
    let answered_while_waiting = host.has_answered(4);
    let prompts_while_waiting = agent_frames(&host.receipt, "session/prompt", None);
    host.release("release");
    let first = host.response(3).await;
    let second = host.response(4).await;
    until_agent_read(&host.receipt, "session/prompt", 2).await;
    let turns = HubTurns::from_observed(&host.hub_events().await);
    let second_prompts = agent_frames(&host.receipt, "session/prompt", Some(SECOND));
    let steers = agent_frames(&host.receipt, "_session/steering", None);
    let cancels = agent_frames(&host.receipt, "session/cancel", None);
    let stopped = host.shutdown().await;

    // Assert: one input waited, under the ID the line returned, and nothing reached the agent.
    assert!(!answered_while_waiting, "never answered busy: {second}");
    assert_eq!(waiting.len(), 1, "one queue_add: {waiting:?}");
    assert_eq!(waiting[0]["preview"], SECOND, "{waiting:?}");
    let input_id = waiting[0]["inputId"].as_str().expect("queued input ID");
    assert_eq!(prompts_while_waiting, 1, "nothing sent while the turn runs");
    // Assert: the input started its own turn, and the response is that turn's end.
    let running_turn = turns.started.first().map(|(turn, _)| turn.as_str());
    let own_turn = turns.started_by(input_id);
    assert!(
        own_turn.is_some() && own_turn != running_turn,
        "the queued input ID starts a new turn: {turns:?}"
    );
    assert_eq!(
        running_turn.and_then(|turn| turns.stop_reason_of(turn)),
        Some(&StopReason::MaxTokens),
        "{turns:?}"
    );
    assert_eq!(
        own_turn.and_then(|turn| turns.stop_reason_of(turn)),
        Some(&StopReason::EndTurn),
        "{turns:?}"
    );
    assert_eq!(first["result"]["stopReason"], "max_tokens", "{first}");
    assert_eq!(
        second["result"]["stopReason"], "end_turn",
        "answered at its own turn's end: {second}"
    );
    assert_eq!(second_prompts, 1, "sent once");
    assert_eq!(steers, 0, "an agent that cannot steer is never asked to");
    assert_eq!(cancels, 0, "no cancel");
    stopped.expect("Host shutdown");
}

/// Oracle: D1 R12 and R14, V22b — "Not sent: an error naming the reason.
/// Zero cancels." The busy prompt waits as the line's head while the turn
/// runs; the agent then exits, the provider retires, and the prompt answers
/// with an error naming providerRetired. It never started: the agent never
/// received it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_busy_prompt_not_sent_when_the_provider_retires() {
    // Arrange: a non-steering agent whose first turn is held, and that exits on request.
    let mut host = AcpBusyHost::start(&BusyAgent::Cursor(&[
        "hold-first-prompt-open",
        "exit-on-stop-file",
    ]))
    .await;
    host.send_prompt(3, FIRST).await;
    until_agent_read(&host.receipt, "session/prompt", 1).await;
    host.send_prompt(4, SECOND).await;
    let waiting = host.until_queued_or_answered(1, 4).await;

    // Act: the agent exits while the busy prompt waits.
    host.release("stop");
    let second = host.response(4).await;
    let second_prompts = agent_frames(&host.receipt, "session/prompt", Some(SECOND));
    let cancels = agent_frames(&host.receipt, "session/cancel", None);
    let stopped = host.shutdown().await;

    // Assert: the retired endpoint's hub refuses observation, so "never
    // started" is read at the agent, which never received the message.
    assert_eq!(waiting.len(), 1, "one queue_add: {waiting:?}");
    let message = second["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("not sent: providerRetired"),
        "an error naming why it was not sent: {second}"
    );
    assert_eq!(
        second_prompts, 0,
        "it never started: the agent never saw it"
    );
    assert_eq!(cancels, 0, "no cancel");
    stopped.expect("Host shutdown");
}

/// Oracle: D1 R12 and R14, V22b — "Not sent: an error naming the reason.
/// Zero cancels." A pre-submit refusal of the line's head: the busy prompt's
/// input, held by the session's worker while the turn runs, is cancelled from
/// the line (`_session/queue/cancel`), so the worker never submits it. The
/// prompt answers at once with an error naming the reason, before the running
/// turn ends; no turn carries its input ID and the agent never saw it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_busy_prompt_cancelled_before_submission_answers_not_sent() {
    // Arrange: a non-steering agent whose first turn is held; the busy prompt waits.
    let mut host = AcpBusyHost::start(&BusyAgent::Cursor(&["hold-first-prompt-open"])).await;
    host.send_prompt(3, FIRST).await;
    until_agent_read(&host.receipt, "session/prompt", 1).await;
    host.send_prompt(4, SECOND).await;
    let waiting = host.until_queued_or_answered(1, 4).await;
    let input_id = waiting
        .first()
        .and_then(|item| item["inputId"].as_str())
        .expect("queued input ID")
        .to_owned();

    // Act: refuse the head before it is submitted, then let the turn end.
    let cancelled = host.queue_cancel(&input_id).await;
    let second = host.response(4).await;
    let turn_still_running = !host.has_answered(3);
    host.release("release");
    let first = host.response(3).await;
    let turns = HubTurns::from_observed(&host.hub_events().await);
    let second_prompts = agent_frames(&host.receipt, "session/prompt", Some(SECOND));
    let cancels = agent_frames(&host.receipt, "session/cancel", None);
    let stopped = host.shutdown().await;

    // Assert
    assert_eq!(cancelled["result"]["cancelled"], true, "{cancelled}");
    let message = second["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("not sent: queueCancelled"),
        "an error naming why it was not sent: {second}"
    );
    assert!(
        turn_still_running,
        "it answers before the running turn ends"
    );
    assert_eq!(first["result"]["stopReason"], "end_turn", "{first}");
    assert_eq!(
        turns.started_by(&input_id),
        None,
        "it never started: {turns:?}"
    );
    assert_eq!(second_prompts, 0, "the agent never saw it");
    assert_eq!(cancels, 0, "no cancel");
    stopped.expect("Host shutdown");
}

/// Oracle: D1 R14 (owner, 16 Nov: "close shoudl just interrupt the agent and
/// close") with D1 R12, V22b: a close requested while a busy ACP prompt waits
/// behind a running turn ends that prompt not sent, reason "session closed",
/// interrupts the running turn, and closes the session. The waiting prompt
/// never reaches the agent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_while_a_busy_prompt_waits_interrupts_and_ends_it_not_sent() {
    // Arrange: a non-steering agent that can close, whose first turn is held.
    let mut host = AcpBusyHost::start(&BusyAgent::Cursor(&[
        "hold-first-prompt-open",
        "advertise-close",
    ]))
    .await;
    host.send_prompt(3, FIRST).await;
    until_agent_read(&host.receipt, "session/prompt", 1).await;
    host.send_prompt(4, SECOND).await;
    let waiting = host.until_queued_or_answered(1, 4).await;

    // Act: close while it waits.
    let closed = host.close().await;
    // Asserted where it's read: a refused close would leave the held turn
    // running and every later wait hanging.
    assert!(closed.is_ok(), "close is admitted mid-turn: {closed:?}");
    let second = host.response(4).await;
    let first = host.response(3).await;
    until_agent_read(&host.receipt, "session/close", 1).await;
    let second_prompts = agent_frames(&host.receipt, "session/prompt", Some(SECOND));
    let cancels = agent_frames(&host.receipt, "session/cancel", None);
    let stopped = host.shutdown().await;

    // Assert
    assert_eq!(waiting.len(), 1, "one queue_add: {waiting:?}");
    let message = second["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("not sent: session closed"),
        "an error naming why it was not sent: {second}"
    );
    assert_eq!(first["result"]["stopReason"], "cancelled", "{first}");
    assert_eq!(second_prompts, 0, "the agent never saw it");
    assert_eq!(cancels, 1, "the running turn is interrupted once");
    stopped.expect("Host shutdown");
}
