#![deny(clippy::wildcard_enum_match_arm)]
//! One direct message's whole life at central: queued until the recipient's
//! Router answers or the cancel mark passes (design 2026-11-17, owner
//! record:218). Its only timed work is the cancel, a due item on the 5-minute
//! grid; delivery and Acks are events. The sender is told when the message did
//! not simply arrive: refused, cancelled, or delivered after a cancel.
use super::timer_effects::{NotUuidV7, queue_message};
use central_engine_model::{
    Context, EngineTime, EventTag, Machine, MachineKind, RefusalTag, StateTag, Transition,
    TransitionError,
};
use workspace_ids::{MessageId, RequestId};
use workspace_model::SessionAddress;
use workspace_model::central_mail as mail;

pub(crate) struct DirectMessage;

/// What the reducer needs to cancel a message and tell its sender.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DirectMessageSubject {
    pub id: MessageId,
    /// None when the sender is a person: there is no session to tell, and the
    /// sender's Router reads the state instead.
    pub sender_session: Option<SessionAddress>,
    pub sender_request_id: RequestId,
    pub cancel_due_at: EngineTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum State {
    Queued(DirectMessageSubject),
    /// Not final: an Ack that was stuck can still arrive and deliver it.
    Cancelled(DirectMessageSubject),
    Delivered,
    Failed(mail::MessageRefusal),
}

impl StateTag for State {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Queued(_) => "queued",
            Self::Cancelled(_) => "cancelled",
            Self::Delivered => "delivered",
            Self::Failed(_) => "failed",
        }
    }
    fn is_terminal(&self) -> bool {
        match self {
            Self::Queued(_) | Self::Cancelled(_) => false,
            Self::Delivered | Self::Failed(_) => true,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Event {
    /// The due batch reached the cancel mark.
    Due { occurrence: EngineTime },
    /// The recipient's Router answered a delivery.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the link's Ack frame drives it in step 2c piece 5"
        )
    )]
    Ack(mail::DeliveryAnswer),
}

impl EventTag for Event {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::Due { .. } => "due",
            Self::Ack(mail::DeliveryAnswer::Accepted) => "accepted",
            Self::Ack(mail::DeliveryAnswer::Refused { .. }) => "refused",
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Effect {
    /// A DeliveryOutcome message from central to the sender's session.
    TellSender(mail::MessageRecord),
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum DirectMessageRefusal {
    /// A repeated Ack, or a refusal of a message already cancelled: nothing
    /// changes and the Ack needs no further work.
    AlreadySettled,
    InvalidIdentifier,
}

impl RefusalTag for DirectMessageRefusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::AlreadySettled => "already_settled",
            Self::InvalidIdentifier => "invalid_identifier",
        }
    }
}

impl From<NotUuidV7> for TransitionError<DirectMessageRefusal> {
    fn from(_: NotUuidV7) -> Self {
        TransitionError::Domain(DirectMessageRefusal::InvalidIdentifier)
    }
}

/// The one place the sender's notice is built; no notice for a person sender.
fn tell_sender(
    subject: &DirectMessageSubject,
    outcome: mail::MessageOutcome,
    context: &mut Context<'_>,
) -> Result<Vec<Effect>, NotUuidV7> {
    let Some(session) = &subject.sender_session else {
        return Ok(vec![]);
    };
    let notice = queue_message(
        context,
        session.clone(),
        mail::MessageBody::DeliveryOutcome {
            message_id: subject.id.clone(),
            request_id: subject.sender_request_id.clone(),
            outcome,
        },
    )?;
    Ok(vec![Effect::TellSender(notice)])
}

impl Machine for DirectMessage {
    const KIND: MachineKind = MachineKind("direct_message");
    type State = State;
    type Event = Event;
    type Effect = Effect;
    type Error = DirectMessageRefusal;
    fn transition(
        state: &State,
        event: Event,
        context: &mut Context<'_>,
    ) -> Result<(State, Vec<Effect>), TransitionError<DirectMessageRefusal>> {
        match (state, event) {
            (State::Queued(subject), Event::Due { occurrence }) => {
                if subject.cancel_due_at != occurrence {
                    return Err(TransitionError::Stale);
                }
                let effects = tell_sender(
                    subject,
                    mail::MessageOutcome::Cancelled {
                        reason: mail::MessageCancellation::NotDeliveredWithin15Minutes,
                    },
                    context,
                )?;
                Ok((State::Cancelled(subject.clone()), effects))
            }
            (State::Queued(_), Event::Ack(mail::DeliveryAnswer::Accepted)) => {
                Ok((State::Delivered, vec![]))
            }
            (State::Queued(subject), Event::Ack(mail::DeliveryAnswer::Refused { reason })) => {
                let effects =
                    tell_sender(subject, mail::MessageOutcome::Refused { reason }, context)?;
                Ok((State::Failed(reason), effects))
            }
            (State::Cancelled(subject), Event::Ack(mail::DeliveryAnswer::Accepted)) => {
                let effects =
                    tell_sender(subject, mail::MessageOutcome::DeliveredAfterAll, context)?;
                Ok((State::Delivered, effects))
            }
            (
                State::Cancelled(_) | State::Delivered | State::Failed(_),
                Event::Ack(mail::DeliveryAnswer::Refused { .. }),
            )
            | (State::Delivered | State::Failed(_), Event::Ack(mail::DeliveryAnswer::Accepted)) => {
                Err(TransitionError::Domain(
                    DirectMessageRefusal::AlreadySettled,
                ))
            }
            (State::Cancelled(_) | State::Delivered | State::Failed(_), Event::Due { .. }) => {
                Err(TransitionError::Stale)
            }
        }
    }
    fn transitions() -> &'static [Transition] {
        &[
            Transition {
                from: "queued",
                event: "due",
                to: "cancelled",
                terminal: false,
            },
            Transition {
                from: "queued",
                event: "accepted",
                to: "delivered",
                terminal: true,
            },
            Transition {
                from: "queued",
                event: "refused",
                to: "failed",
                terminal: true,
            },
            Transition {
                from: "cancelled",
                event: "accepted",
                to: "delivered",
                terminal: true,
            },
        ]
    }
    fn next_due(state: &State) -> Option<EngineTime> {
        match state {
            State::Queued(subject) => Some(subject.cancel_due_at),
            State::Cancelled(_) | State::Delivered | State::Failed(_) => None,
        }
    }
}

#[cfg(test)]
#[path = "direct_message_test.rs"]
mod tests;
