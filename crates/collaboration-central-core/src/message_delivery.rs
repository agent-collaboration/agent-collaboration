#![deny(clippy::wildcard_enum_match_arm)]
//! Central delivering direct messages to the recipient's Router (2c, Lead A
//! design 2026-11-17). Each owed delivery runs the merged owed-reply machine:
//! sent at once over the recipient's live connection while its window has
//! room (at most 256 in flight per connection), settled by that connection's
//! Ack, and reset to waiting when the Router reconnects (the only resend, owner
//! record:299). The sender's Router is told how each message ended, once.
//!
//! The 15-minute cancel (owner, record:358) is not here: messages never arm
//! central's alarm (record:278), and how the cancel fires is with the owner.
//!
//! Draft behind a storage seam: `OwedDeliveries` is where the owed-delivery
//! table plugs in once the owner picks its shape; connections are ids the host
//! mints per accepted socket (after #302).
use crate::engine::machines::owed_reply::{Event, Finish, OwedReply, RouterAnswer, State};
use central_engine_model::{Context, EngineTime, Machine};
use uuid::Uuid;
use workspace_model::{MachineId, MessageId, RequestId, SessionAddress};

/// At most this many deliveries in flight per connection (Lead A).
pub const DELIVERY_WINDOW: usize = 256;

/// One direct message central owes a recipient Router.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwedDelivery {
    pub message_id: MessageId,
    /// The sender's request, so the sender's Router can match the outcome.
    pub request_id: RequestId,
    pub sender: SessionAddress,
    pub recipient: MachineId,
    pub accepted_at: EngineTime,
    pub state: State,
}

/// The storage seam: open deliveries by recipient, in acceptance order.
pub trait OwedDeliveries {
    fn get(&self, message_id: &MessageId) -> Option<OwedDelivery>;
    /// Every delivery not yet done or cancelled, for `recipient`, oldest first.
    fn open_for(&self, recipient: &MachineId) -> Vec<OwedDelivery>;
    fn put(&mut self, delivery: OwedDelivery);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryEvent {
    /// Central accepted a direct message; `connection` is the recipient's
    /// live connection, if any.
    Accepted {
        delivery: OwedDelivery,
        connection: Option<Uuid>,
    },
    /// The recipient's Router connected (its Welcome was sent).
    Connected {
        recipient: MachineId,
        connection: Uuid,
    },
    /// The recipient's Router answered one send.
    Acked {
        recipient: MachineId,
        connection: Uuid,
        message_id: MessageId,
        send_id: Uuid,
        answer: RouterAnswer,
    },
}

/// How a message ended, for the sender's Router.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryEnd {
    Delivered,
    Refused,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryEffect {
    /// Send `Deliver { message, send_id }` on this connection.
    Send {
        connection: Uuid,
        message_id: MessageId,
        send_id: Uuid,
    },
    /// Tell the sender's Router (`MessageOutcome`).
    TellSender {
        sender: SessionAddress,
        request_id: RequestId,
        message_id: MessageId,
        end: DeliveryEnd,
    },
}

/// Applies one event to the owed deliveries and returns what to send.
pub fn plan_delivery(
    deliveries: &mut impl OwedDeliveries,
    event: DeliveryEvent,
    context: &mut Context<'_>,
) -> Vec<DeliveryEffect> {
    let mut effects = Vec::new();
    match event {
        DeliveryEvent::Accepted {
            delivery,
            connection,
        } => {
            let recipient = delivery.recipient.clone();
            deliveries.put(delivery);
            if let Some(connection) = connection {
                fill_window(deliveries, &recipient, connection, context, &mut effects);
            }
        }
        DeliveryEvent::Connected {
            recipient,
            connection,
        } => {
            for mut delivery in deliveries.open_for(&recipient) {
                if let Ok((state, _)) = OwedReply::transition(
                    &delivery.state,
                    Event::Reconnected { connection },
                    context,
                ) {
                    delivery.state = state;
                    deliveries.put(delivery);
                }
            }
            fill_window(deliveries, &recipient, connection, context, &mut effects);
        }
        DeliveryEvent::Acked {
            recipient,
            connection,
            message_id,
            send_id,
            answer,
        } => {
            let Some(mut delivery) = deliveries.get(&message_id) else {
                return effects;
            };
            if delivery.recipient != recipient {
                return effects;
            }
            let answered = OwedReply::transition(
                &delivery.state,
                Event::Answer {
                    send_id,
                    connection,
                    answer,
                },
                context,
            );
            // A late or stale answer (another send, another connection) changes nothing.
            if let Ok((state, _)) = answered {
                delivery.state = state;
                effects.push(tell_sender(&delivery));
                deliveries.put(delivery);
                fill_window(deliveries, &recipient, connection, context, &mut effects);
            }
        }
    }
    effects
}

/// Sends waiting deliveries for `recipient` on `connection` while it has room.
fn fill_window(
    deliveries: &mut impl OwedDeliveries,
    recipient: &MachineId,
    connection: Uuid,
    context: &mut Context<'_>,
    effects: &mut Vec<DeliveryEffect>,
) {
    let open = deliveries.open_for(recipient);
    let mut in_flight = open
        .iter()
        .filter(|delivery| {
            matches!(delivery.state, State::InFlight { connection: on, .. } if on == connection)
        })
        .count();
    for mut delivery in open {
        if in_flight >= DELIVERY_WINDOW {
            break;
        }
        if !matches!(delivery.state, State::Waiting) {
            continue;
        }
        let Ok((state, _)) =
            OwedReply::transition(&delivery.state, Event::Send { connection }, context)
        else {
            continue;
        };
        if let State::InFlight { send_id, .. } = state {
            effects.push(DeliveryEffect::Send {
                connection,
                message_id: delivery.message_id.clone(),
                send_id,
            });
            in_flight += 1;
        }
        delivery.state = state;
        deliveries.put(delivery);
    }
}

fn tell_sender(delivery: &OwedDelivery) -> DeliveryEffect {
    let end = match delivery.state {
        State::Done {
            finish: Finish::Delivered,
        } => DeliveryEnd::Delivered,
        State::Done {
            finish: Finish::Refused,
        } => DeliveryEnd::Refused,
        State::Expired | State::Waiting | State::InFlight { .. } => DeliveryEnd::Cancelled,
    };
    DeliveryEffect::TellSender {
        sender: delivery.sender.clone(),
        request_id: delivery.request_id.clone(),
        message_id: delivery.message_id.clone(),
        end,
    }
}

#[cfg(test)]
#[path = "message_delivery_test.rs"]
mod tests;
