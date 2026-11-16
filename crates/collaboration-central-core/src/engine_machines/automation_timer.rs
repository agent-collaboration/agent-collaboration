#![deny(clippy::wildcard_enum_match_arm)]
use super::timer_effects::{TimerRefusal, queue_message};
use crate::central_timer_plans::AutomationRow;
use central_engine_model::{
    Context, EngineTime, EventTag, Machine, MachineKind, StateTag, Transition, TransitionError,
};
use workspace_model::central_mail as mail;

pub(crate) struct AutomationTimer;
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum State {
    Active(AutomationRow),
    Done { evaluated: Option<EngineTime> },
    Disabled { evaluated: Option<EngineTime> },
}

#[cfg(test)]
#[path = "automation_timer_test.rs"]
mod tests;
impl StateTag for State {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Active(_) => "active",
            Self::Done { .. } => "done",
            Self::Disabled { .. } => "disabled",
        }
    }
    fn is_terminal(&self) -> bool {
        match self {
            Self::Active(_) => false,
            Self::Done { .. } | Self::Disabled { .. } => true,
        }
    }
}
#[derive(Clone)]
pub(crate) enum Event {
    Due { occurrence: EngineTime },
    Disable,
    Discard,
}
impl EventTag for Event {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::Due { .. } => "due",
            Self::Disable => "disable",
            Self::Discard => "discard",
        }
    }
}
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Effect {
    QueueMessage(mail::MessageRecord),
    ScheduleAt(EngineTime),
    ClearSchedule,
}

impl Machine for AutomationTimer {
    const KIND: MachineKind = MachineKind("automation_timer");
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
                if row.due != Some(occurrence) {
                    return Err(TransitionError::Stale);
                }
                let through = occurrence.max(context.now);
                let due = row
                    .timing
                    .next_due(row.anchor, Some(through))
                    .map_err(|_| TransitionError::Domain(TimerRefusal::InvalidTiming))?
                    .filter(|due| *due <= row.expires_at);
                if due.is_some_and(|due| !central_engine_model::supported_time(due)) {
                    return Err(TransitionError::Domain(TimerRefusal::InvalidTiming));
                }
                let message = queue_message(
                    context,
                    row.target.clone(),
                    mail::MessageBody::Automation {
                        automation_id: row.id.clone(),
                        text: row.text.clone(),
                    },
                )?;
                let (next, schedule) = match due {
                    Some(due) => {
                        let mut row = row.clone();
                        row.evaluated = Some(through);
                        row.due = Some(due);
                        (State::Active(row), Effect::ScheduleAt(due))
                    }
                    None => (
                        State::Done {
                            evaluated: Some(through),
                        },
                        Effect::ClearSchedule,
                    ),
                };
                Ok((next, vec![Effect::QueueMessage(message), schedule]))
            }
            (State::Active(row), Event::Disable | Event::Discard) => Ok((
                State::Disabled {
                    evaluated: row.evaluated,
                },
                vec![Effect::ClearSchedule],
            )),
            (State::Done { .. } | State::Disabled { .. }, Event::Due { .. }) => {
                Err(TransitionError::Stale)
            }
            (State::Done { .. } | State::Disabled { .. }, Event::Disable | Event::Discard) => {
                Err(TransitionError::Domain(TimerRefusal::AlreadyFinished))
            }
        }
    }
    fn transitions() -> &'static [Transition] {
        &[
            Transition {
                from: "active",
                event: "due",
                to: "active",
                terminal: false,
            },
            Transition {
                from: "active",
                event: "due",
                to: "done",
                terminal: true,
            },
            Transition {
                from: "active",
                event: "disable",
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
            State::Active(row) => row.due,
            State::Done { .. } | State::Disabled { .. } => None,
        }
    }
}
