use super::ProviderCallbackClaims;
use agent_client_protocol::schema::v1::RequestId;
use hegel::generators as gs;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Callback {
    id: u8,
    method: &'static str,
}

#[derive(Default)]
struct ClaimScenario {
    claims: ProviderCallbackClaims,
    in_flight: BTreeMap<Callback, RequestId>,
    written: Vec<RequestId>,
}

impl ClaimScenario {
    fn arrive(&mut self, callbacks: &[Callback], batch: bool) {
        let mut newly_claimed = BTreeSet::new();
        let expected: Vec<_> = callbacks
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, callback)| {
                !self.in_flight.contains_key(callback) && newly_claimed.insert(*callback)
            })
            .collect();
        let incoming = encode_frame(
            callbacks
                .iter()
                .enumerate()
                .map(|(arrival, callback)| {
                    json!({
                        "jsonrpc": "2.0", "id": callback.id, "method": callback.method,
                        "params": {"arrival": arrival}
                    })
                })
                .collect(),
            batch,
        );
        let dispatched = self.claims.incoming(incoming.to_string());
        if expected.is_empty() {
            assert_eq!(
                dispatched, None,
                "an in-flight callback dispatched a second time"
            );
            return;
        }
        let actual: Value =
            serde_json::from_str(&dispatched.expect("a new callback must dispatch"))
                .expect("valid dispatched frame");
        let actual = if batch {
            actual.as_array().expect("preserved batch").clone()
        } else {
            vec![actual]
        };
        assert_eq!(
            actual.len(),
            expected.len(),
            "one SDK request per new claim"
        );
        for (message, (arrival, callback)) in actual.into_iter().zip(expected) {
            assert_eq!(message["method"], callback.method);
            assert_eq!(message["params"], json!({"arrival": arrival}));
            let sdk_id: RequestId =
                serde_json::from_value(message["id"].clone()).expect("typed SDK correlation ID");
            assert!(
                !self.in_flight.values().any(|active| active == &sdk_id),
                "separate in-flight callbacks need separate SDK slots"
            );
            self.in_flight.insert(callback, sdk_id);
        }
    }

    fn write_replies(&mut self, limit: usize, batch: bool) {
        let pending: Vec<_> = self
            .in_flight
            .iter()
            .filter(|(_, sdk_id)| !self.written.contains(sdk_id))
            .take(limit)
            .collect();
        if pending.is_empty() {
            return;
        }
        let reply =
            |id: Value, method| json!({"jsonrpc":"2.0", "id":id, "result":{"method":method}});
        let sdk_replies = encode_frame(
            pending
                .iter()
                .map(|(peer, sdk_id)| reply(json!(sdk_id), peer.method))
                .collect(),
            batch,
        );
        let expected = encode_frame(
            pending
                .iter()
                .map(|(peer, _)| reply(json!(peer.id), peer.method))
                .collect(),
            batch,
        );
        let (wire_replies, tokens) = self.claims.outgoing(sdk_replies.to_string());
        let wire_replies: Value = serde_json::from_str(&wire_replies).expect("valid wire replies");
        assert_eq!(
            wire_replies, expected,
            "one reply with the original ID for each SDK request"
        );
        assert_eq!(
            tokens,
            pending
                .into_iter()
                .map(|(_, sdk_id)| sdk_id.clone())
                .collect::<Vec<_>>()
        );
        self.written.extend(tokens);
    }

    fn flush(&mut self) {
        let written = std::mem::take(&mut self.written);
        self.claims.flushed(written.clone());
        self.in_flight.retain(|_, sdk_id| !written.contains(sdk_id));
    }
}

fn encode_frame(messages: Vec<Value>, batch: bool) -> Value {
    if batch {
        Value::Array(messages)
    } else {
        messages.into_iter().next().expect("one message")
    }
}

fn callback(case: &hegel::TestCase) -> Callback {
    Callback {
        id: case.draw(gs::integers::<u8>().max_value(4)),
        method: match case.draw(gs::integers::<u8>().max_value(2)) {
            0 => "session/request_permission",
            1 => "cursor/ask_question",
            _ => "elicitation/create",
        },
    }
}

#[hegel::test]
fn generated_requests_and_flushes_claim_each_in_flight_callback_once(case: hegel::TestCase) {
    let mut scenario = ClaimScenario::default();
    let first = Callback {
        method: "session/request_permission",
        ..callback(&case)
    };
    let sibling = Callback {
        method: "cursor/ask_question",
        ..first
    };
    scenario.arrive(&[first, first, sibling], true);
    scenario.write_replies(2, true);
    scenario.arrive(&[first], false);
    scenario.flush();
    scenario.arrive(&[first], false);

    for _ in 0..case.draw(gs::integers::<u8>().min_value(5).max_value(75)) {
        match case.draw(gs::integers::<u8>().max_value(4)) {
            0 => scenario.arrive(&[callback(&case)], false),
            1 => {
                let batch: Vec<_> = (0..case.draw(gs::integers::<u8>().min_value(2).max_value(5)))
                    .map(|_| callback(&case))
                    .collect();
                scenario.arrive(&batch, true);
            }
            2 => scenario.write_replies(1, false),
            3 => scenario.write_replies(
                usize::from(case.draw(gs::integers::<u8>().min_value(2).max_value(5))),
                true,
            ),
            _ => scenario.flush(),
        }
    }
    scenario.write_replies(usize::MAX, true);
    scenario.flush();
    assert!(scenario.in_flight.is_empty());
}
