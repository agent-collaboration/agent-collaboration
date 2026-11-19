//! A `session/prompt` on the Router's ACP socket for a session that is in a
//! turn (D1 R12, Change 4 "The Router's ACP socket does the same"): it is
//! steered into the running turn, or queued when the agent can't steer, and
//! never refused as busy. Races are bounded to one retry each; a second
//! contention queues, and an unknown steer outcome is never re-sent.
//!
//! This module only decides where the message went; the session route owns
//! the ACP frames and streams the turn.

use collaboration_contracts::{
    CommandFailure, PromptSessionCommand, QueueInputCommand, QueuedInputOutcome,
    SessionCommandPort, SessionEventAttachment, SessionSteerOutcome, SteerSessionCommand,
};
use message_board::{Identity, SessionRef};
use session_event_model::{InputId, PromptContent, ProviderSubmission, SessionEvent};

/// A prompt that found its session in a turn.
pub(crate) struct BusyPrompt<'a> {
    pub(crate) commands: &'a dyn SessionCommandPort,
    pub(crate) session: SessionRef,
    pub(crate) content: Vec<PromptContent>,
    pub(crate) actor: Identity,
    pub(crate) attachment: &'a SessionEventAttachment,
}

/// Where a busy prompt's message went.
#[derive(Debug)]
pub(crate) enum BusyPromptPlacement {
    /// The message is part of this turn: steered into it, started by it, or
    /// taken from the session's line by it. Its answer is that turn's.
    InTurn { turn_id: String },
    /// The command was refused.
    Refused(CommandFailure),
    /// The agent took the steer but reported it failed.
    SteerFailed { reason: String },
    /// It waited in the line and never ran.
    NotSent { reason: String },
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

/// Steers, prompts or queues the message, and says where it went once the
/// turn that takes it has started.
pub(crate) async fn place_busy_prompt(prompt: BusyPrompt<'_>) -> BusyPromptPlacement {
    let BusyPrompt {
        commands,
        session,
        content,
        actor,
        attachment,
    } = prompt;
    let submission = ProviderSubmission::plain(content);
    let mut next = match running_turn(attachment) {
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
                ) => return BusyPromptPlacement::InTurn { turn_id },
                // The turn ended before the steer; nothing was injected.
                Ok(SessionSteerOutcome::PromptRequired) => Next::Prompt,
                Ok(SessionSteerOutcome::Failed { reason }) => {
                    return BusyPromptPlacement::SteerFailed { reason };
                }
                Err(CommandFailure::Unsupported | CommandFailure::UnsupportedOperation { .. }) => {
                    Next::Queue
                }
                // The expected turn changed: look once more.
                Err(CommandFailure::Busy) if !last => match running_turn(attachment) {
                    Some(turn_id) => Next::Steer(turn_id),
                    None => Next::Prompt,
                },
                Err(CommandFailure::Busy) => Next::Queue,
                Err(error) => return BusyPromptPlacement::Refused(error),
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
                    return BusyPromptPlacement::InTurn {
                        turn_id: turn.turn_id,
                    };
                }
                // A turn started first: steer or queue it once.
                Err(CommandFailure::Busy) if !last => match running_turn(attachment) {
                    Some(turn_id) => Next::Steer(turn_id),
                    None => Next::Queue,
                },
                Err(CommandFailure::Busy) => Next::Queue,
                Err(error) => return BusyPromptPlacement::Refused(error),
            },
            Next::Queue => break,
        };
    }
    queue(commands, session, submission, actor).await
}

async fn queue(
    commands: &dyn SessionCommandPort,
    session: SessionRef,
    submission: ProviderSubmission,
    actor: Identity,
) -> BusyPromptPlacement {
    let queued = match commands
        .queue_add(QueueInputCommand {
            target: session.clone(),
            submission,
            actor,
        })
        .await
    {
        Ok(queued) => queued,
        Err(error) => return BusyPromptPlacement::Refused(error),
    };
    // queue_contents assigns the input ID; follow the one it returned.
    let Ok(input_id) = InputId::try_from(queued.input_id) else {
        return BusyPromptPlacement::NotSent {
            reason: "queued input has an invalid ID".to_owned(),
        };
    };
    match commands.queue_outcome(session, input_id).await {
        Ok(QueuedInputOutcome::Started { turn_id }) => BusyPromptPlacement::InTurn { turn_id },
        Ok(QueuedInputOutcome::NotSent { reason }) => BusyPromptPlacement::NotSent { reason },
        Err(error) => BusyPromptPlacement::Refused(error),
    }
}
