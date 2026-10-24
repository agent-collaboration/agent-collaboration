#![allow(clippy::unwrap_used)]
//! The registry lookup carries distinct service and peer identities through real frames.
use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol as wire;
use serde_json::{Value, json};

const FRAME: &str = "019a1234-5678-7000-8000-000000000002";
const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const PEER: &str = "019a1234-5678-7000-8000-000000000073";

fn read_request(scope: Value) -> Value {
    json!({"kind":"request","value":{"frame":FRAME,"body":{"kind":"consumer","value":{
        "kind":"read","value":{"scope":scope,"cursor":null}
    }}}})
}

fn read_answer(lookup: Value) -> Value {
    json!({"kind":"answer","value":{"frame":FRAME,"body":{"kind":"consumer","value":{
        "kind":"read","value":{"kind":"installationPeer","value":lookup}
    }}}})
}

#[test]
fn installation_peer_request_preserves_the_service_identity_without_casting_it() {
    let expected = read_request(json!({"kind":"installationPeer","value":{"service":SERVICE}}));
    let decoded: wire::ClientFrame<BoardConsumerProtocol> =
        serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
}

#[test]
fn installation_peer_answers_preserve_the_enrolled_peer_or_definite_unknown() {
    for lookup in [
        json!({"kind":"enrolled","value":{"peer":PEER}}),
        json!({"kind":"unknown"}),
    ] {
        let expected = read_answer(lookup);
        let decoded: wire::ServerFrame<BoardConsumerProtocol> =
            serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
    }
}

#[test]
fn installation_peer_request_refuses_missing_invalid_and_extra_fields() {
    for scope in [
        json!({"kind":"installationPeer","value":{}}),
        json!({"kind":"installationPeer","value":{"service":"invalid"}}),
        json!({"kind":"installationPeer","value":{"service":SERVICE,"peer":PEER}}),
    ] {
        assert!(
            serde_json::from_value::<wire::ClientFrame<BoardConsumerProtocol>>(read_request(
                scope.clone()
            ))
            .is_err(),
            "accepted invalid installation lookup: {scope}"
        );
    }
}

#[test]
fn installation_peer_answer_refuses_missing_peers_cast_service_ids_and_unknown_states() {
    for lookup in [
        json!({"kind":"enrolled","value":{}}),
        json!({"kind":"enrolled","value":{"peer":SERVICE}}),
        json!({"kind":"enrolled","value":{"peer":PEER,"service":SERVICE}}),
        json!({"kind":"unknown","value":{"peer":PEER}}),
        json!({"kind":"revoked"}),
    ] {
        assert!(
            serde_json::from_value::<wire::ServerFrame<BoardConsumerProtocol>>(read_answer(
                lookup.clone()
            ))
            .is_err(),
            "accepted invalid installation answer: {lookup}"
        );
    }
}
