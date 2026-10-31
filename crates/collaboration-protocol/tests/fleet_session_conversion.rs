//! Fleet registration uses the existing mail boundary without changing provider identities.
use collaboration_protocol::{EndpointId, SessionId, SessionRef, UuidIdentity};
use proptest::prelude::*;
use serde_json::{Value, json};

#[cfg(test)]
fn assert_lossless_session(wire: Value) {
    let router: SessionRef = serde_json::from_value(wire.clone()).unwrap();
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
    assert_eq!(SessionRef::try_from(fleet).unwrap(), router);
}

#[test]
fn existing_uuid_versions_variants_and_utf8_boundaries_survive_both_conversions() {
    for version in "0123456789abcdef".chars() {
        for variant in "0123456789abcdef".chars() {
            for endpoint in ["a".to_owned(), "a".repeat(64)] {
                for session in ["a".to_owned(), "😀".repeat(1024)] {
                    assert_lossless_session(json!({
                        "endpoint": {
                            "serviceId": format!("00000000-0000-{version}000-{variant}000-000000000001"),
                            "endpointId": endpoint
                        },
                        "sessionId": session
                    }));
                }
            }
        }
    }
    for service in [
        "00000000-0000-0000-0000-000000000000",
        "ffffffff-ffff-ffff-ffff-ffffffffffff",
    ] {
        assert_lossless_session(json!({
            "endpoint": {"serviceId": service, "endpointId": "codex-local"},
            "sessionId": "provider:opaque/thread#1"
        }));
    }
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
        serde_json::from_str(include_str!("fixtures/fleet_identity_negatives.json")).unwrap();
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
    fn service_identity_acceptance_agrees(input in any::<String>()) {
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
    fn arbitrary_valid_provider_identity_round_trips(session in "[^\\x00]{1,256}", endpoint in "[a-z][a-z0-9-]{0,63}", service in "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}") {
        assert_lossless_session(json!({"endpoint":{"serviceId":service,"endpointId":endpoint},"sessionId":session}));
    }
}
