#![deny(clippy::wildcard_enum_match_arm)]
//! One reply a Router is owed: sent over a live connection with window room,
//! settled by that connection's answer, reset when the Router reconnects, and
//! dropped when retention ends. Traffic drives it; it never arms the alarm.
use central_engine_model::{
    Context, EngineTime, EventTag, Machine, MachineKind, RefusalTag, StateTag, Transition,
    TransitionError,
};
use uuid::Uuid;

pub struct OwedReply;

#[cfg(test)]
#[path = "owed_reply_test.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Finish {
    Delivered,
    Refused,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum State {
    Waiting,
    InFlight { send_id: Uuid, connection: Uuid },
    Done { finish: Finish },
    Expired,
}
impl StateTag for State {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::InFlight { .. } => "in_flight",
            Self::Done { .. } => "done",
            Self::Expired => "expired",
        }
    }
    fn is_terminal(&self) -> bool {
        match self {
            Self::Waiting | Self::InFlight { .. } => false,
            Self::Done { .. } | Self::Expired => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouterAnswer {
    Accepted,
    RefusedForGood,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "event",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Event {
    /// The Router is connected on `connection` and its window has room.
    Send {
        connection: Uuid,
    },
    Answer {
        send_id: Uuid,
        connection: Uuid,
        answer: RouterAnswer,
    },
    /// The Router reconnected; `connection` is the new connection's id.
    Reconnected {
        connection: Uuid,
    },
    RetentionEnded,
}
impl EventTag for Event {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::Send { .. } => "send",
            Self::Answer { .. } => "answer",
            Self::Reconnected { .. } => "reconnected",
            Self::RetentionEnded => "retention_ended",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    SendItem { send_id: Uuid, connection: Uuid },
    Finish(Finish),
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum OwedReplyRefusal {
    AlreadyInFlight,
}
impl RefusalTag for OwedReplyRefusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::AlreadyInFlight => "already_in_flight",
        }
    }
}

fn finish_for(answer: RouterAnswer) -> Finish {
    match answer {
        RouterAnswer::Accepted => Finish::Delivered,
        RouterAnswer::RefusedForGood => Finish::Refused,
    }
}

impl Machine for OwedReply {
    const KIND: MachineKind = MachineKind("owed_reply");
    type State = State;
    type Event = Event;
    type Effect = Effect;
    type Error = OwedReplyRefusal;
    fn transition(
        state: &State,
        event: Event,
        context: &mut Context<'_>,
    ) -> Result<(State, Vec<Effect>), TransitionError<OwedReplyRefusal>> {
        match (state, event) {
            (State::Waiting, Event::Send { connection }) => {
                let send_id = context.ids.next_id();
                Ok((
                    State::InFlight {
                        send_id,
                        connection,
                    },
                    vec![Effect::SendItem {
                        send_id,
                        connection,
                    }],
                ))
            }
            (
                State::InFlight {
                    send_id: in_flight_send,
                    connection: in_flight_connection,
                },
                Event::Answer {
                    send_id,
                    connection,
                    answer,
                },
            ) => {
                if *in_flight_send != send_id || *in_flight_connection != connection {
                    return Err(TransitionError::Stale);
                }
                let finish = finish_for(answer);
                Ok((State::Done { finish }, vec![Effect::Finish(finish)]))
            }
            (State::InFlight { .. }, Event::Reconnected { .. }) => Ok((State::Waiting, vec![])),
            (State::InFlight { .. }, Event::Send { .. }) => {
                Err(TransitionError::Domain(OwedReplyRefusal::AlreadyInFlight))
            }
            (State::Waiting | State::InFlight { .. }, Event::RetentionEnded) => {
                Ok((State::Expired, vec![]))
            }
            (State::Waiting, Event::Answer { .. } | Event::Reconnected { .. }) => {
                Err(TransitionError::Stale)
            }
            (
                State::Done { .. } | State::Expired,
                Event::Send { .. }
                | Event::Answer { .. }
                | Event::Reconnected { .. }
                | Event::RetentionEnded,
            ) => Err(TransitionError::Stale),
        }
    }
    fn transitions() -> &'static [Transition] {
        &[
            Transition {
                from: "waiting",
                event: "send",
                to: "in_flight",
                terminal: false,
            },
            Transition {
                from: "in_flight",
                event: "answer",
                to: "done",
                terminal: true,
            },
            Transition {
                from: "in_flight",
                event: "reconnected",
                to: "waiting",
                terminal: false,
            },
            Transition {
                from: "waiting",
                event: "retention_ended",
                to: "expired",
                terminal: true,
            },
            Transition {
                from: "in_flight",
                event: "retention_ended",
                to: "expired",
                terminal: true,
            },
        ]
    }
    fn next_due(state: &State) -> Option<EngineTime> {
        match state {
            State::Waiting | State::InFlight { .. } | State::Done { .. } | State::Expired => None,
        }
    }
}
