#![deny(clippy::wildcard_enum_match_arm)]
use crate::rows::AutomationRow;
use central_engine_model::{
    Context, EngineTime, EventTag, Machine, MachineKind, StateTag, Transition, TransitionError,
};
use fleet_timer_policy::TimerRefusal;
use workspace_model::central_mail as mail;

pub struct AutomationTimer;
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum State {
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
pub enum Event {
    Due { occurrence: EngineTime },
    Disable,
}
impl EventTag for Event {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::Due { .. } => "due",
            Self::Disable => "disable",
        }
    }
}
#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(
        clippy::large_enum_variant,
        reason = "effects are built and applied once per transition"
    )
)]
pub enum Effect {
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
                    mail::MessageBody::Automation {
                        automation_id: row.id.clone(),
                        text: row.text.clone(),
                    },
                    context.now,
                );
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
            (State::Active(row), Event::Disable) => Ok((
                State::Disabled {
                    evaluated: row.evaluated,
                },
                vec![Effect::ClearSchedule],
            )),
            (State::Done { .. } | State::Disabled { .. }, Event::Due { .. }) => {
                Err(TransitionError::Stale)
            }
            (State::Done { .. } | State::Disabled { .. }, Event::Disable) => {
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
        ]
    }
    fn next_due(state: &State) -> Option<EngineTime> {
        match state {
            State::Active(row) => row.due,
            State::Done { .. } | State::Disabled { .. } => None,
        }
    }
}
