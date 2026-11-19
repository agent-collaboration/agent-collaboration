//! Fleet registration uses the existing mail boundary without changing provider identities.
use collaboration_protocol::{EndpointId, HarnessSessionRef, SessionId, UuidIdentity};
use proptest::prelude::*;
use serde_json::{Value, json};

#[test]
fn exported_identity_patterns_reject_a_trailing_linefeed() {
    let uuid = "00000000-0000-4000-8000-000000000001\n";
    assert!(UuidIdentity::try_from(uuid.to_owned()).is_err());
    let uuid_schema = serde_json::to_value(schemars::schema_for!(UuidIdentity)).unwrap();
    assert!(
        !jsonschema::validator_for(&uuid_schema)
            .unwrap()
            .is_valid(&json!(uuid))
    );
    let endpoint = "codex-local\n";
    assert!(EndpointId::try_from(endpoint.to_owned()).is_err());
    let endpoint_schema = serde_json::to_value(schemars::schema_for!(EndpointId)).unwrap();
    assert!(
        !jsonschema::validator_for(&endpoint_schema)
            .unwrap()
            .is_valid(&json!(endpoint))
    );
}

#[cfg(test)]
fn assert_lossless_session(wire: Value) {
    let router: HarnessSessionRef = serde_json::from_value(wire.clone()).unwrap();
    let fleet: message_board::SessionRef = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&router).unwrap(), wire);
    assert_eq!(serde_json::to_value(&fleet).unwrap(), wire);
    assert_eq!(
        serde_json::to_vec(&router).unwrap(),
        serde_json::to_vec(&fleet).unwrap()
    );
    assert_eq!(
        message_board::SessionRef::try_from(router.clone()).unwrap(),
        fleet
    );
    assert_eq!(HarnessSessionRef::try_from(fleet).unwrap(), router);
}

fn assert_legacy_session_is_refused(wire: Value) -> serde_json::Result<()> {
    let fleet: message_board::SessionRef = serde_json::from_value(wire.clone())?;
    assert!(serde_json::from_value::<HarnessSessionRef>(wire).is_err());
    assert!(HarnessSessionRef::try_from(fleet).is_err());
    Ok(())
}

#[test]
fn only_machine_uuid_v7_crosses_the_harness_boundary_and_provider_ids_round_trip()
-> serde_json::Result<()> {
    for version in "0123456789abcdef".chars() {
        for variant in "0123456789abcdef".chars() {
            for endpoint in ["a".to_owned(), "a".repeat(64)] {
                for session in ["a".to_owned(), "😀".repeat(1024)] {
                    let wire = json!({
                        "endpoint": {
                            "serviceId": format!("00000000-0000-{version}000-{variant}000-000000000001"),
                            "endpointId": endpoint
                        },
                        "sessionId": session
                    });
                    if version == '7' && matches!(variant, '8' | '9' | 'a' | 'b') {
                        assert_lossless_session(wire);
                    } else {
                        assert_legacy_session_is_refused(wire)?;
                    }
                }
            }
        }
    }
    for service in [
        "00000000-0000-0000-0000-000000000000",
        "ffffffff-ffff-ffff-ffff-ffffffffffff",
    ] {
        assert_legacy_session_is_refused(json!({
            "endpoint": {"serviceId": service, "endpointId": "codex-local"},
            "sessionId": "provider:opaque/thread#1"
        }))?;
    }
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(tag = "field", content = "input", rename_all = "camelCase")]
enum IdentityBoundaryNegative {
    #[serde(rename = "sessionId")]
    Session(String),
    #[serde(rename = "endpointId")]
    Endpoint(String),
    #[serde(rename = "serviceId")]
    Service(String),
}

#[test]
fn both_domains_reject_invalid_identity_boundaries_without_normalizing() {
    let cases: Vec<IdentityBoundaryNegative> =
        serde_json::from_str(include_str!("../fixtures/fleet_identity_negatives.json")).unwrap();
    for case in cases {
        match case {
            IdentityBoundaryNegative::Session(input) => {
                assert!(SessionId::try_from(input.clone()).is_err());
                assert!(message_board::SessionId::try_from(input).is_err());
            }
            IdentityBoundaryNegative::Endpoint(input) => {
                assert!(EndpointId::try_from(input.clone()).is_err());
                assert!(message_board::EndpointId::try_from(input).is_err());
            }
            IdentityBoundaryNegative::Service(input) => {
                assert!(UuidIdentity::try_from(input.clone()).is_err());
                assert!(message_board::ServiceId::try_from(input).is_err());
            }
        }
    }
}

proptest! {
    #[test]
    fn service_identity_acceptance_agrees(input in prop_oneof![
        any::<String>(),
        "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
        "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
    ]) {
        prop_assert_eq!(UuidIdentity::try_from(input.clone()).is_ok(), message_board::ServiceId::try_from(input).is_ok());
    }

    #[test]
    fn endpoint_identity_acceptance_agrees(input in any::<String>()) {
        prop_assert_eq!(EndpointId::try_from(input.clone()).is_ok(), message_board::EndpointId::try_from(input).is_ok());
    }

    #[test]
    fn opaque_session_identity_acceptance_agrees(input in any::<String>()) {
        prop_assert_eq!(SessionId::try_from(input.clone()).is_ok(), message_board::SessionId::try_from(input).is_ok());
    }

    #[test]
    fn arbitrary_valid_provider_identity_round_trips(session in "[^\\x00]{1,256}", endpoint in "[a-z][a-z0-9-]{0,63}", service in "[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}") {
        assert_lossless_session(json!({"endpoint":{"serviceId":service,"endpointId":endpoint},"sessionId":session}));
    }
}
