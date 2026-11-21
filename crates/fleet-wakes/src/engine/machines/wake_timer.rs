#![deny(clippy::wildcard_enum_match_arm)]
use crate::rows::WakeRow;
use central_engine_model::{
    Context, EngineTime, EventTag, Machine, MachineKind, StateTag, Transition, TransitionError,
};
use fleet_timer_policy::TimerRefusal;
use workspace_model::central_mail as mail;

pub struct WakeTimer;
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum State {
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
pub enum Event {
    Due { occurrence: EngineTime },
    Cancel,
}
impl EventTag for Event {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::Due { .. } => "due",
            Self::Cancel => "cancel",
        }
    }
}
#[derive(Debug, Eq, PartialEq)]
pub enum Effect {
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
                let raw_id = context.ids.next_id();
                if raw_id.get_version_num() != 7 {
                    return Err(TransitionError::Domain(TimerRefusal::InvalidIdentifier));
                }
                let message_id = raw_id
                    .to_string()
                    .try_into()
                    .map_err(|_| TransitionError::Domain(TimerRefusal::InvalidIdentifier))?;
                let message = mail::MessageRecord::queued_central(
                    message_id,
                    row.target.clone(),
                    mail::MessageBody::Wake {
                        wake_id: row.id.clone(),
                        text: row.text.clone(),
                    },
                    context.now,
                );
                Ok((State::Done, vec![Effect::QueueMessage(message)]))
            }
            (State::Active(_), Event::Cancel) => Ok((State::Disabled, vec![])),
            (State::Done | State::Disabled, Event::Due { .. }) => Err(TransitionError::Stale),
            (State::Done | State::Disabled, Event::Cancel) => {
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
        ]
    }
    fn next_due(state: &State) -> Option<EngineTime> {
        match state {
            State::Active(row) => Some(row.due),
            State::Done | State::Disabled => None,
        }
    }
}
