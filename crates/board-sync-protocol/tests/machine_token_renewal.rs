use board_sync_protocol::{BoardConsumerAnswer, BoardConsumerRequest};
use fleet_identity_protocol::{MachineTokenRenewal, MachineTokenRenewalRefusal, RenewMachineToken};
use serde_json::json;

#[test]
fn renewal_uses_the_real_shared_shapes_in_both_consumer_envelopes() {
    let request = BoardConsumerRequest::RenewMachineToken(RenewMachineToken {
        token: "fixture-access-token".into(),
    });
    let request_wire = json!({"kind":"renewMachineToken","value":{"token":"fixture-access-token"}});
    assert_eq!(serde_json::to_value(&request).unwrap(), request_wire);
    assert!(matches!(
        serde_json::from_value::<BoardConsumerRequest>(request_wire).unwrap(),
        BoardConsumerRequest::RenewMachineToken(_)
    ));

    let answer = BoardConsumerAnswer::RenewMachineToken(MachineTokenRenewal::Refused {
        reason: MachineTokenRenewalRefusal::WrongClient {},
    });
    let answer_wire = json!({"kind":"renewMachineToken","value":{"result":"refused","reason":{"kind":"wrongClient"}}});
    assert_eq!(serde_json::to_value(&answer).unwrap(), answer_wire);
    assert_eq!(
        serde_json::from_value::<BoardConsumerAnswer>(answer_wire).unwrap(),
        answer
    );
}

#[test]
fn renewal_refuses_unknown_nested_fields() {
    for bad in [
        json!({"kind":"renewMachineToken","value":{"token":"fixture","unexpected":true}}),
        json!({"kind":"renewMachineToken","value":{"token":"fixture"},"unexpected":true}),
    ] {
        assert!(serde_json::from_value::<BoardConsumerRequest>(bad).is_err());
    }
}

#[test]
fn renewal_debug_never_contains_its_token() {
    let renewal = RenewMachineToken {
        token: "fixture-secret-canary".into(),
    };
    assert_eq!(format!("{renewal:?}"), "RenewMachineToken([REDACTED])");
}
