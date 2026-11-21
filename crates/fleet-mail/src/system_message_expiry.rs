//! A queued notice or delivery outcome reaches its delivery deadline here.
use central_engine_model::{
    Context, EngineTime, EventTag, Machine, MachineKind, RefusalTag, StateTag, Transition,
    TransitionError, TransitionResult,
};
use std::convert::Infallible;

pub struct SystemMessageExpiry;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum State {
    Queued { deadline: EngineTime },
    Cancelled,
}

impl StateTag for State {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Queued { .. } => "queued",
            Self::Cancelled => "cancelled",
        }
    }
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

#[derive(Clone, Debug)]
pub struct ExpireAt(pub EngineTime);

impl EventTag for ExpireAt {
    fn event_tag(&self) -> &'static str {
        "expire"
    }
}

#[derive(Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ExpiryRefusal {}

impl RefusalTag for ExpiryRefusal {
    fn refusal_tag(&self) -> &'static str {
        match *self {}
    }
}

impl Machine for SystemMessageExpiry {
    const KIND: MachineKind = MachineKind("system_message_expiry");
    type State = State;
    type Event = ExpireAt;
    type Effect = Infallible;
    type Error = ExpiryRefusal;

    fn transition(
        state: &State,
        event: ExpireAt,
        context: &mut Context<'_>,
    ) -> TransitionResult<Self> {
        match state {
            State::Queued { deadline } if *deadline == event.0 && *deadline <= context.now => {
                Ok((State::Cancelled, vec![]))
            }
            State::Queued { .. } | State::Cancelled => Err(TransitionError::Stale),
        }
    }

    fn transitions() -> &'static [Transition] {
        &[Transition {
            from: "queued",
            event: "expire",
            to: "cancelled",
            terminal: true,
        }]
    }

    fn next_due(state: &State) -> Option<EngineTime> {
        match state {
            State::Queued { deadline } => Some(*deadline),
            State::Cancelled => None,
        }
    }
}

#[cfg(test)]
#[path = "system_message_expiry_test.rs"]
mod tests;
