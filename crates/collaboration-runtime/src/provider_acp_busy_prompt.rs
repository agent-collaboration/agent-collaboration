//! A `session/prompt` on the Router's ACP socket for a session that is in a
//! turn (D1 R12, Change 4 "The Router's ACP socket does the same"): it is
//! steered into the running turn, or queued when the agent can't steer, and
//! never refused as busy. Races are bounded to one retry each; a second
//! contention queues, and an unknown steer outcome is never re-sent.

use crate::provider_acp_event_projection::stream_prompt;
use crate::provider_acp_session_route::{command_failure, failure};
use collaboration_contracts::{
    CommandFailure, PromptSessionCommand, QueueInputCommand, QueuedInputOutcome,
    SessionCommandPort, SessionEventAttachment, SessionSteerOutcome, SteerSessionCommand,
};
use message_board::{Identity, SessionRef};
use serde_json::Value;
use session_event_model::{InputId, PromptContent, ProviderSubmission, SessionEvent};
use std::sync::Arc;
use tokio::sync::watch;

/// A prompt that found its session in a turn.
pub(crate) struct BusyPrompt {
    pub(crate) commands: Arc<dyn SessionCommandPort>,
    pub(crate) output: codex_acp_adapter::AcpOutputSender,
    pub(crate) id: Value,
    pub(crate) session: SessionRef,
    pub(crate) content: Vec<PromptContent>,
    pub(crate) actor: Identity,
    pub(crate) attachment: SessionEventAttachment,
    pub(crate) observed_turn: watch::Receiver<Option<String>>,
}

/// The turn running in a session, from its event snapshot.
fn running_turn(attachment: &SessionEventAttachment) -> Option<String> {
    attachment
        .snapshot
        .iter()
        .fold(None, |current, event| match &event.event {
            SessionEvent::TurnStarted { turn_id, .. } => Some(turn_id.clone()),
            SessionEvent::TurnEnded { turn_id, .. } if current.as_deref() == Some(turn_id) => None,
            _ => current,
        })
}

/// What the next step does with the message.
enum Next {
    Steer(String),
    Prompt,
    Queue,
}

/// Answers the prompt when the turn that takes its message ends.
pub(crate) async fn answer_busy_prompt(prompt: BusyPrompt) {
    let BusyPrompt {
        commands,
        output,
        id,
        session,
        content,
        actor,
        attachment,
        observed_turn,
    } = prompt;
    let submission = ProviderSubmission::plain(content);
    let mut next = match running_turn(&attachment) {
        Some(turn_id) => Next::Steer(turn_id),
        None => Next::Prompt,
    };
    // One retry per race: the second contention queues.
    for attempt in 0..2 {
        let last = attempt == 1;
        next = match next {
            Next::Steer(expected_turn_id) => match commands
                .steer(SteerSessionCommand {
                    target: session.clone(),
                    input_id: InputId::generate(),
                    expected_turn_id,
                    submission: submission.clone(),
                    actor: actor.clone(),
                })
                .await
            {
                Ok(
                    SessionSteerOutcome::Injected { turn_id }
                    | SessionSteerOutcome::StartedNewTurn { turn_id },
                ) => {
                    stream_prompt(output, id, turn_id, attachment, observed_turn).await;
                    return;
                }
                // The turn ended before the steer; nothing was injected.
                Ok(SessionSteerOutcome::PromptRequired) => Next::Prompt,
                Ok(SessionSteerOutcome::Failed { reason }) => {
                    let _sent = output.send(failure(id, -32000, &reason)).await;
                    return;
                }
                Err(CommandFailure::Unsupported | CommandFailure::UnsupportedOperation { .. }) => {
                    Next::Queue
                }
                // The expected turn changed: look once more.
                Err(CommandFailure::Busy) if !last => match running_turn(&attachment) {
                    Some(turn_id) => Next::Steer(turn_id),
                    None => Next::Prompt,
                },
                Err(CommandFailure::Busy) => Next::Queue,
                Err(error) => {
                    let _sent = output.send(command_failure(id, error)).await;
                    return;
                }
            },
            Next::Prompt => match commands
                .prompt(PromptSessionCommand {
                    target: session.clone(),
                    input_id: InputId::generate(),
                    submission: submission.clone(),
                    actor: actor.clone(),
                })
                .await
            {
                Ok(turn) => {
                    stream_prompt(output, id, turn.turn_id, attachment, observed_turn).await;
                    return;
                }
                // A turn started first: steer or queue it once.
                Err(CommandFailure::Busy) if !last => match running_turn(&attachment) {
                    Some(turn_id) => Next::Steer(turn_id),
                    None => Next::Queue,
                },
                Err(CommandFailure::Busy) => Next::Queue,
                Err(error) => {
                    let _sent = output.send(command_failure(id, error)).await;
                    return;
                }
            },
            Next::Queue => break,
        };
    }
    queue_and_answer(
        commands,
        output,
        id,
        session,
        submission,
        actor,
        attachment,
        observed_turn,
    )
    .await;
}

#[expect(
    clippy::too_many_arguments,
    reason = "The queued prompt keeps every piece of its ACP request until it answers."
)]
async fn queue_and_answer(
    commands: Arc<dyn SessionCommandPort>,
    output: codex_acp_adapter::AcpOutputSender,
    id: Value,
    session: SessionRef,
    submission: ProviderSubmission,
    actor: Identity,
    attachment: SessionEventAttachment,
    observed_turn: watch::Receiver<Option<String>>,
) {
    let queued = match commands
        .queue_add(QueueInputCommand {
            target: session.clone(),
            submission,
            actor,
        })
        .await
    {
        Ok(queued) => queued,
        Err(error) => {
            let _sent = output.send(command_failure(id, error)).await;
            return;
        }
    };
    // queue_contents assigns the input ID; stream by the one it returned.
    let Ok(input_id) = InputId::try_from(queued.input_id) else {
        let _sent = output
            .send(failure(id, -32000, "queued input has an invalid ID"))
            .await;
        return;
    };
    match commands.queue_outcome(session, input_id).await {
        Ok(QueuedInputOutcome::Started { turn_id }) => {
            stream_prompt(output, id, turn_id, attachment, observed_turn).await;
        }
        Ok(QueuedInputOutcome::NotSent { reason }) => {
            let _sent = output
                .send(failure(id, -32000, &format!("not sent: {reason}")))
                .await;
        }
        Err(error) => {
            let _sent = output.send(command_failure(id, error)).await;
        }
    }
}
