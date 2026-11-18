use super::*;
use central_engine_model::IdSource;
use hegel::{TestCase, generators as gs};
use std::collections::{BTreeMap, BTreeSet};

/// The in-memory owed deliveries (the stand-in for the owed-delivery table).
#[derive(Default)]
struct MemoryDeliveries(BTreeMap<MessageId, OwedDelivery>);

impl OwedDeliveries for MemoryDeliveries {
    fn get(&self, message_id: &MessageId) -> Option<OwedDelivery> {
        self.0.get(message_id).cloned()
    }
    fn open_for(&self, recipient: &MachineId) -> Vec<OwedDelivery> {
        self.open_for_any()
            .into_iter()
            .filter(|delivery| &delivery.recipient == recipient)
            .collect()
    }
    fn put(&mut self, delivery: OwedDelivery) {
        self.0.insert(delivery.message_id.clone(), delivery);
    }
}

impl MemoryDeliveries {
    /// Every open delivery, for any recipient.
    fn open_for_any(&self) -> Vec<OwedDelivery> {
        self.0
            .values()
            .filter(|delivery| matches!(delivery.state, State::Waiting | State::InFlight { .. }))
            .cloned()
            .collect()
    }
}

struct CountingIds(u64);
impl IdSource for CountingIds {
    fn next_id(&mut self) -> Uuid {
        self.0 += 1;
        uuid(0x5000 + self.0)
    }
}

fn uuid(suffix: u64) -> Uuid {
    format!("01890f2e-7b4c-7cc0-98c4-{suffix:012x}")
        .parse()
        .unwrap()
}
fn recipient() -> MachineId {
    uuid(0xb0b).to_string().parse().unwrap()
}
fn sender() -> SessionAddress {
    serde_json::from_value(serde_json::json!({
        "machineId": uuid(0xa11).to_string(), "endpoint": "codex-local", "sessionId": "sender"
    }))
    .unwrap()
}
fn at(seconds: i64) -> EngineTime {
    EngineTime::from_timestamp(1_893_456_000 + seconds, 0).unwrap()
}
fn delivery(seed: u64, accepted: i64) -> OwedDelivery {
    OwedDelivery {
        message_id: uuid(0x1000 + seed).to_string().parse().unwrap(),
        request_id: uuid(0x2000 + seed).to_string().parse().unwrap(),
        sender: sender(),
        recipient: recipient(),
        accepted_at: at(accepted),
        state: State::Waiting,
    }
}

struct Central {
    deliveries: MemoryDeliveries,
    ids: CountingIds,
    now: i64,
}

impl Central {
    fn new() -> Self {
        Self {
            deliveries: MemoryDeliveries::default(),
            ids: CountingIds(0),
            now: 0,
        }
    }
    fn apply(&mut self, event: DeliveryEvent) -> Vec<DeliveryEffect> {
        plan_delivery(
            &mut self.deliveries,
            event,
            &mut Context {
                now: at(self.now),
                ids: &mut self.ids,
            },
        )
    }
}

fn sends(effects: &[DeliveryEffect]) -> Vec<(MessageId, Uuid)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            DeliveryEffect::Send {
                message_id,
                send_id,
                ..
            } => Some((message_id.clone(), *send_id)),
            DeliveryEffect::TellSender { .. } => None,
        })
        .collect()
}

fn ends(effects: &[DeliveryEffect]) -> Vec<(MessageId, DeliveryEnd)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            DeliveryEffect::TellSender {
                message_id, end, ..
            } => Some((message_id.clone(), *end)),
            DeliveryEffect::Send { .. } => None,
        })
        .collect()
}

#[test]
fn a_message_goes_at_once_to_a_connected_recipient_and_its_ack_tells_the_sender() {
    let mut central = Central::new();
    let message = delivery(1, 0);
    let connection = uuid(0xc1);

    let sent = central.apply(DeliveryEvent::Accepted {
        delivery: message.clone(),
        connection: Some(connection),
    });
    let Ok([(message_id, send_id)]) = <[_; 1]>::try_from(sends(&sent)) else {
        panic!("one send: {sent:?}");
    };
    let acked = central.apply(DeliveryEvent::Acked {
        recipient: recipient(),
        connection,
        message_id: message_id.clone(),
        send_id,
        answer: RouterAnswer::Accepted,
    });

    assert_eq!(message_id, message.message_id);
    assert_eq!(
        ends(&acked),
        vec![(message.message_id, DeliveryEnd::Delivered)]
    );
}

#[test]
fn only_a_reconnect_resends_and_the_old_send_answer_is_stale() {
    let mut central = Central::new();
    let message = delivery(2, 0);
    let first = uuid(0xc1);
    let second = uuid(0xc2);
    let sent = central.apply(DeliveryEvent::Accepted {
        delivery: message.clone(),
        connection: Some(first),
    });
    let Ok([(_, first_send)]) = <[_; 1]>::try_from(sends(&sent)) else {
        panic!("one send");
    };

    let resent = central.apply(DeliveryEvent::Connected {
        recipient: recipient(),
        connection: second,
    });
    let stale = central.apply(DeliveryEvent::Acked {
        recipient: recipient(),
        connection: first,
        message_id: message.message_id.clone(),
        send_id: first_send,
        answer: RouterAnswer::Accepted,
    });
    let Ok([(_, second_send)]) = <[_; 1]>::try_from(sends(&resent)) else {
        panic!("one resend on the new connection: {resent:?}");
    };
    let settled = central.apply(DeliveryEvent::Acked {
        recipient: recipient(),
        connection: second,
        message_id: message.message_id.clone(),
        send_id: second_send,
        answer: RouterAnswer::RefusedForGood,
    });

    assert_ne!(first_send, second_send);
    assert!(
        stale.is_empty(),
        "an answer to the earlier send settles nothing"
    );
    assert_eq!(
        ends(&settled),
        vec![(message.message_id, DeliveryEnd::Refused)]
    );
}

#[test]
fn at_most_256_deliveries_are_in_flight_per_connection_and_acks_refill_the_window() {
    let mut central = Central::new();
    let connection = uuid(0xc4);
    for seed in 0..300 {
        central.apply(DeliveryEvent::Accepted {
            delivery: delivery(100 + seed, 0),
            connection: None,
        });
    }

    let first = central.apply(DeliveryEvent::Connected {
        recipient: recipient(),
        connection,
    });
    let (message_id, send_id) = sends(&first)[0].clone();
    let refill = central.apply(DeliveryEvent::Acked {
        recipient: recipient(),
        connection,
        message_id,
        send_id,
        answer: RouterAnswer::Accepted,
    });

    assert_eq!(sends(&first).len(), DELIVERY_WINDOW);
    assert_eq!(sends(&refill).len(), 1, "one answered, one more sent");
}

/// Random acceptance, reconnects, answers and time against the planner.
struct DeliveryModel {
    central: Central,
    connection: Option<Uuid>,
    next_seed: u64,
    told: BTreeMap<MessageId, usize>,
    in_flight: BTreeMap<MessageId, (Uuid, Uuid)>,
    accepted: BTreeSet<MessageId>,
}

impl DeliveryModel {
    fn record(&mut self, effects: &[DeliveryEffect]) {
        for effect in effects {
            match effect {
                DeliveryEffect::Send {
                    connection,
                    message_id,
                    send_id,
                } => {
                    self.in_flight
                        .insert(message_id.clone(), (*connection, *send_id));
                }
                DeliveryEffect::TellSender { message_id, .. } => {
                    *self.told.entry(message_id.clone()).or_default() += 1;
                    self.in_flight.remove(message_id);
                }
            }
        }
    }
}

#[hegel::state_machine]
impl DeliveryModel {
    #[rule(weight = 3)]
    fn accept(&mut self, _: TestCase) {
        let message = delivery(self.next_seed, self.central.now);
        self.next_seed += 1;
        self.accepted.insert(message.message_id.clone());
        let effects = self.central.apply(DeliveryEvent::Accepted {
            delivery: message,
            connection: self.connection,
        });
        self.record(&effects);
    }

    #[rule]
    fn reconnect(&mut self, _: TestCase) {
        let connection = uuid(0xc000 + self.next_seed);
        self.next_seed += 1;
        self.connection = Some(connection);
        self.in_flight.clear();
        let effects = self.central.apply(DeliveryEvent::Connected {
            recipient: recipient(),
            connection,
        });
        self.record(&effects);
    }

    #[rule]
    fn disconnect(&mut self, _: TestCase) {
        self.connection = None;
    }

    #[rule(weight = 3)]
    fn answer(&mut self, tc: TestCase) {
        let candidates: Vec<(MessageId, (Uuid, Uuid))> = self
            .in_flight
            .iter()
            .map(|(id, sent)| (id.clone(), *sent))
            .collect();
        tc.assume(!candidates.is_empty());
        let (message_id, (connection, send_id)) = tc.draw(gs::sampled_from(candidates));
        let answer = if tc.draw(gs::booleans()) {
            RouterAnswer::Accepted
        } else {
            RouterAnswer::RefusedForGood
        };
        let effects = self.central.apply(DeliveryEvent::Acked {
            recipient: recipient(),
            connection,
            message_id,
            send_id,
            answer,
        });
        self.record(&effects);
    }

    #[invariant(always_run)]
    fn the_window_holds_and_each_sender_is_told_at_most_once(&self, _: TestCase) {
        let mut per_connection: BTreeMap<Uuid, usize> = BTreeMap::new();
        for delivery in self.central.deliveries.open_for_any() {
            if let State::InFlight { connection, .. } = delivery.state {
                *per_connection.entry(connection).or_default() += 1;
            }
        }
        assert!(
            per_connection
                .values()
                .all(|count| *count <= DELIVERY_WINDOW)
        );
        assert!(
            self.told.values().all(|times| *times == 1),
            "{:?}",
            self.told
        );
        let ended: BTreeSet<&MessageId> = self.told.keys().collect();
        let open: BTreeSet<MessageId> = self
            .central
            .deliveries
            .open_for_any()
            .into_iter()
            .map(|delivery| delivery.message_id)
            .collect();
        for id in &self.accepted {
            assert!(
                ended.contains(id) != open.contains(id),
                "{id:?} is either open or ended with its sender told, never both or neither"
            );
        }
    }
}

#[hegel::test(test_cases = 60)]
fn deliveries_keep_the_window_and_end_once(tc: TestCase) {
    hegel::stateful::machine(DeliveryModel {
        central: Central::new(),
        connection: None,
        next_seed: 0,
        told: BTreeMap::new(),
        in_flight: BTreeMap::new(),
        accepted: BTreeSet::new(),
    })
    .run(tc);
}
