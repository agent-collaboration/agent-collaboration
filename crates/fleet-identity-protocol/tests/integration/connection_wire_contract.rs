use fleet_identity_protocol::{Heartbeat, MachineCapability};
use serde_json::json;

#[test]
fn heartbeat_capability_uses_the_adopted_kind_tag() {
    let heartbeat = Heartbeat {
        capabilities: vec![MachineCapability::RemoteStart {}],
    };
    let expected = json!({"capabilities":[{"kind":"remoteStart"}]});
    assert_eq!(serde_json::to_value(&heartbeat).unwrap(), expected);
    assert_eq!(
        serde_json::from_value::<Heartbeat>(expected).unwrap(),
        heartbeat
    );
}

#[test]
fn heartbeat_refuses_unknown_body_fields_and_capabilities() {
    for invalid in [
        json!({"capabilities":[],"extra":true}),
        json!({"capabilities":[{"kind":"futureCapability"}]}),
        json!({"capabilities":["RemoteStart"]}),
    ] {
        assert!(
            serde_json::from_value::<Heartbeat>(invalid.clone()).is_err(),
            "accepted {invalid}"
        );
    }
}
