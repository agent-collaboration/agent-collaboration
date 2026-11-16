#![deny(clippy::wildcard_enum_match_arm)]
use super::timer_effects::{TimerRefusal, queue_message};
use crate::central_timer_plans::WakeRow;
use central_engine_model::{
    Context, EngineTime, EventTag, Machine, MachineKind, StateTag, Transition, TransitionError,
};
use workspace_model::central_mail as mail;

pub(crate) struct WakeTimer;
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum State {
    Active(WakeRow),
    Done,
    Disabled,
}

#[cfg(test)]
#[path = "wake_timer_test.rs"]
mod tests;
impl StateTag for State {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Active(_) => "active",
            Self::Done => "done",
            Self::Disabled => "disabled",
        }
    }
    fn is_terminal(&self) -> bool {
        match self {
            Self::Active(_) => false,
            Self::Done | Self::Disabled => true,
        }
    }
}
#[derive(Clone)]
pub(crate) enum Event {
    Due { occurrence: EngineTime },
    Cancel,
    Discard,
}
impl EventTag for Event {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::Due { .. } => "due",
            Self::Cancel => "cancel",
            Self::Discard => "discard",
        }
    }
}
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Effect {
    QueueMessage(mail::MessageRecord),
}

impl Machine for WakeTimer {
    const KIND: MachineKind = MachineKind("wake_timer");
    type State = State;
    type Event = Event;
    type Effect = Effect;
    type Error = TimerRefusal;
    fn transition(
        state: &State,
        event: Event,
        context: &mut Context<'_>,
    ) -> Result<(State, Vec<Effect>), TransitionError<TimerRefusal>> {
        match (state, event) {
            (State::Active(row), Event::Due { occurrence }) => {
                if row.due != occurrence {
                    return Err(TransitionError::Stale);
                }
                let message = queue_message(
                    context,
                    row.target.clone(),
                    mail::MessageBody::Wake {
                        wake_id: row.id.clone(),
                        text: row.text.clone(),
                    },
                )?;
                Ok((State::Done, vec![Effect::QueueMessage(message)]))
            }
            (State::Active(_), Event::Cancel | Event::Discard) => Ok((State::Disabled, vec![])),
            (State::Done | State::Disabled, Event::Due { .. }) => Err(TransitionError::Stale),
            (State::Done | State::Disabled, Event::Cancel | Event::Discard) => {
                Err(TransitionError::Domain(TimerRefusal::AlreadyFinished))
            }
        }
    }
    fn transitions() -> &'static [Transition] {
        &[
            Transition {
                from: "active",
                event: "due",
                to: "done",
                terminal: true,
            },
            Transition {
                from: "active",
                event: "cancel",
                to: "disabled",
                terminal: true,
            },
            Transition {
                from: "active",
                event: "discard",
                to: "disabled",
                terminal: true,
            },
        ]
    }
    fn next_due(state: &State) -> Option<EngineTime> {
        match state {
            State::Active(row) => Some(row.due),
            State::Done | State::Disabled => None,
        }
    }
}
