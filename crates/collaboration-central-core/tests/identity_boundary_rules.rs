#[cfg(test)]
#[path = "support/schema_validation.rs"]
mod schema_validation;

use collaboration_central_core::identity_contract::{
    ConnectionView, DecideDeviceLogin, MachineEnrollment, MachinePresenceAnswer,
    PersonAdmissionRequest, TokenLifetimeSeconds,
};
use fleet_identity_protocol::ReportSessionActivity;
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

#[cfg(test)]
fn assert_refused<T: DeserializeOwned + JsonSchema>(value: Value) {
    assert!(
        serde_json::from_value::<T>(value.clone()).is_err(),
        "serde accepted {value}"
    );
    let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap();
    assert!(
        !schema_validation::contract_validator(&schema)
            .unwrap()
            .is_valid(&value),
        "schema accepted {value}"
    );
}

#[cfg(test)]
fn assert_null_read_omitted_write<T: DeserializeOwned + Serialize + JsonSchema>(
    mut value: Value,
    field: &str,
) {
    let expected = value.clone();
    value[field] = Value::Null;
    let parsed: T = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), expected);
    let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap();
    assert!(
        schema_validation::contract_validator(&schema)
            .unwrap()
            .is_valid(&value)
    );
}

#[test]
fn internal_enrollments_never_become_public_states() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("../contracts/MachineEnrollment.fixtures.json")).unwrap();
    for state in ["reserved", "approved"] {
        let mut invalid = fixtures[0].clone();
        invalid["state"] = json!(state);
        assert_refused::<MachineEnrollment>(invalid);
    }
    for valid in fixtures {
        let mut missing_client = valid;
        missing_client
            .as_object_mut()
            .unwrap()
            .remove("machineClientId");
        assert_refused::<MachineEnrollment>(missing_client);
    }
    let schema = serde_json::to_value(schemars::schema_for!(MachineEnrollment)).unwrap();
    let mut states = schema["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variant| variant["properties"]["state"]["const"].as_str().unwrap())
        .collect::<Vec<_>>();
    states.sort_unstable();
    assert_eq!(states, ["enrolled", "expired", "revoked"]);
}

#[test]
fn optional_reads_accept_null_but_writes_omit_the_key() {
    assert_null_read_omitted_write::<MachinePresenceAnswer>(
        json!({"state":"notConnected"}),
        "closedAgoMs",
    );
    assert_null_read_omitted_write::<ConnectionView>(json!({"state":"notConnected"}), "lastSeenAt");
    assert_null_read_omitted_write::<PersonAdmissionRequest>(
        json!({"operation":"admitSignIn","organizationId":"019a3dce-2000-7000-8000-000000000001","personId":"019a3dce-2000-7000-8000-000000000002"}),
        "verifiedEmail",
    );
    let decisions: Vec<Value> =
        serde_json::from_str(include_str!("../contracts/DecideDeviceLogin.fixtures.json")).unwrap();
    assert_null_read_omitted_write::<DecideDeviceLogin>(
        decisions[0].clone(),
        "approverSecondFactorAt",
    );
    let enrollments: Vec<Value> =
        serde_json::from_str(include_str!("../contracts/MachineEnrollment.fixtures.json")).unwrap();
    for index in [0, 2, 4] {
        assert_null_read_omitted_write::<MachineEnrollment>(enrollments[index].clone(), "endsAt");
    }
}

#[test]
fn javascript_counters_refuse_two_to_the_fifty_third() {
    let activity: Value = serde_json::from_str(include_str!(
        "../../fleet-identity-protocol/contracts/ReportSessionActivity.fixtures.json"
    ))
    .unwrap();
    let mut invalid = activity[0].clone();
    invalid["sequence"] = json!(9007199254740992_u64);
    assert!(serde_json::from_value::<ReportSessionActivity>(invalid.clone()).is_err());
    let schema = serde_json::to_value(schemars::schema_for!(ReportSessionActivity)).unwrap();
    assert!(
        !schema_validation::contract_validator(&schema)
            .unwrap()
            .is_valid(&invalid)
    );
    let presence: Value = serde_json::from_str(include_str!(
        "../contracts/MachinePresenceAnswer.fixtures.json"
    ))
    .unwrap();
    let mut invalid = presence[0].clone();
    invalid["attachSequence"] = json!(9007199254740992_u64);
    assert!(serde_json::from_value::<MachinePresenceAnswer>(invalid.clone()).is_err());
    let schema = serde_json::to_value(schemars::schema_for!(MachinePresenceAnswer)).unwrap();
    assert!(
        !schema_validation::contract_validator(&schema)
            .unwrap()
            .is_valid(&invalid)
    );
}

#[test]
fn configured_token_lifetime_requires_the_adopted_range() {
    for invalid in [0, 59, 3601] {
        assert_refused::<TokenLifetimeSeconds>(json!(invalid));
    }
}

#[test]
fn reviewed_scalar_claim_schema_rejects_invalid_installation() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!(
        "../contracts/MachineTokenClaims.fixtures.json"
    ))
    .unwrap();
    let mut invalid = fixtures[0].clone();
    invalid["principal"]["installation_id"] = json!("x".repeat(36));
    assert_refused::<collaboration_central_core::identity_contract::MachineTokenClaims>(invalid);
}

#[test]
fn resource_name_schema_refuses_blank_and_noncanonical_text() {
    let schema = serde_json::to_value(schemars::schema_for!(message_board::ResourceName)).unwrap();
    let validator = schema_validation::contract_validator(&schema).unwrap();
    for blank in [" ", "\t\n", "\u{0085}", "\u{2003}"] {
        assert!(message_board::ResourceName::try_from(blank.to_owned()).is_err());
        assert!(!validator.is_valid(&json!(blank)));
    }
    for padded in [
        " a ".to_owned(),
        "a\r".to_owned(),
        "\ta".to_owned(),
        format!(" {} ", "a".repeat(256)),
    ] {
        let parsed = message_board::ResourceName::try_from(padded.clone()).unwrap();
        assert!(!validator.is_valid(&json!(padded)));
        assert!(validator.is_valid(&serde_json::to_value(parsed).unwrap()));
    }
    for canonical in ["Workspace", "a\0b", "\u{feff}", "\u{feff}a", "line\n\ttwo"] {
        let parsed = message_board::ResourceName::try_from(canonical.to_owned()).unwrap();
        assert_eq!(parsed.as_str(), canonical);
        assert!(validator.is_valid(&json!(canonical)));
    }
}
