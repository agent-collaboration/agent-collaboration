#[cfg(test)]
#[path = "support/schema_validation.rs"]
mod schema_validation;

use fleet_identity_protocol::{
    AbsolutePath, CommitId, IsolationInstanceId, IsolationKind, LaunchContext, MachineEnvironment,
    ModelName, RepositoryLocator,
};
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

#[cfg(test)]
fn assert_rejected<T: DeserializeOwned + Serialize + JsonSchema>(value: Value) {
    assert!(
        serde_json::from_value::<T>(value.clone()).is_err(),
        "serde accepted {value}"
    );
    let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap();
    let validator = schema_validation::contract_validator(&schema).unwrap();
    assert!(!validator.is_valid(&value), "schema accepted {value}");
    assert!(validator.validate(&value).is_err());
}

#[cfg(test)]
fn assert_accepted<T: DeserializeOwned + JsonSchema>(value: Value) {
    assert!(serde_json::from_value::<T>(value.clone()).is_ok());
    let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap();
    let validator = schema_validation::contract_validator(&schema).unwrap();
    assert!(validator.is_valid(&value));
    assert!(validator.validate(&value).is_ok());
}

#[test]
fn new_identifiers_require_uuid_v7_and_canonical_lowercase() {
    for invalid in [
        "00000000-0000-4000-8000-000000000001",
        "019A3DCE-2000-7000-8000-000000000001",
        "019a3dce-2000-7000-0000-000000000001",
    ] {
        assert_rejected::<IsolationInstanceId>(json!(invalid));
    }
}

#[test]
fn commit_identifiers_require_exact_lowercase_hex_lengths() {
    for length in [39, 41, 63, 65] {
        assert_rejected::<CommitId>(json!("a".repeat(length)));
    }
    for value in ["A".repeat(40), "z".repeat(64)] {
        assert_rejected::<CommitId>(json!(value));
    }
}

#[test]
fn launch_strings_reject_the_byte_bound_overflow() {
    assert!(RepositoryLocator::try_from("r".repeat(2048)).is_ok());
    assert_rejected::<RepositoryLocator>(json!("r".repeat(2049)));
    assert!(ModelName::try_from("m".repeat(256)).is_ok());
    assert_rejected::<ModelName>(json!("m".repeat(257)));
    assert!(AbsolutePath::try_from(format!("/{}", "a".repeat(4095))).is_ok());
    for invalid in [
        "relative".to_owned(),
        "/nul\0".to_owned(),
        format!("/{}", "a".repeat(4096)),
    ] {
        assert_rejected::<AbsolutePath>(json!(invalid));
    }
}

#[test]
fn isolation_and_environment_tags_are_closed_and_case_sensitive() {
    for invalid in ["Host", "process-sandbox", "unknown"] {
        assert_rejected::<IsolationKind>(json!(invalid));
    }
    for invalid in ["Physical", "cloud-vm", "unknown"] {
        assert_rejected::<MachineEnvironment>(json!(invalid));
    }
}

#[test]
fn unknown_launch_metadata_reads_null_and_writes_no_key() {
    for field in ["repository", "commit", "model"] {
        let mut wire = json!({"workingDirectory":"/work/item-two"});
        wire[field] = Value::Null;
        let context: LaunchContext = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(context).unwrap(),
            json!({"workingDirectory":"/work/item-two"})
        );
        let schema = serde_json::to_value(schemars::schema_for!(LaunchContext)).unwrap();
        assert!(
            schema_validation::contract_validator(&schema)
                .unwrap()
                .is_valid(&wire)
        );
    }
}

#[test]
fn multibyte_limits_count_utf8_bytes_in_both_serde_and_schema() {
    assert_accepted::<ModelName>(json!("é".repeat(128)));
    assert_accepted::<RepositoryLocator>(json!("é".repeat(1024)));
    assert_accepted::<AbsolutePath>(json!(format!("/{}a", "é".repeat(2047))));
    assert_rejected::<ModelName>(json!(format!("{}a", "é".repeat(128))));
    assert_rejected::<RepositoryLocator>(json!(format!("{}a", "é".repeat(1024))));
    assert_rejected::<AbsolutePath>(json!(format!("/{}", "é".repeat(2048))));
}

#[test]
fn borrowed_machine_labels_and_session_ids_keep_their_byte_limits() {
    assert_accepted::<message_board::MachineLabel>(json!("é".repeat(128)));
    assert_rejected::<message_board::MachineLabel>(json!(format!("{}a", "é".repeat(128))));
    let mut session = json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"é".repeat(2048)});
    assert_accepted::<message_board::SessionRef>(session.clone());
    session["sessionId"] = json!(format!("{}a", "é".repeat(2048)));
    assert_rejected::<message_board::SessionRef>(session);
}

#[test]
fn reviewed_scalar_installation_schema_rejects_non_uuid_text() {
    assert_rejected::<fleet_identity_protocol::InstallationId>(json!("x".repeat(36)));
}

#[test]
fn reviewed_scalar_endpoint_schema_rejects_invalid_syntax() {
    for invalid in ["Codex", "9"] {
        assert_rejected::<message_board::EndpointId>(json!(invalid));
    }
}

#[test]
fn reviewed_scalar_session_schema_rejects_nul() {
    assert_rejected::<message_board::SessionId>(json!("a\0b"));
}

#[test]
fn reviewed_scalar_machine_label_schema_rejects_controls_and_blank_text() {
    for invalid in [
        "a\0b",
        "a\u{0008}b",
        "a\u{001f}b",
        " \t\n",
        "\u{0085}\u{2003}",
    ] {
        assert_rejected::<message_board::MachineLabel>(json!(invalid));
    }
}

#[test]
fn reviewed_scalar_schemas_preserve_valid_constructor_values() {
    for uuid in [
        "00000000-0000-0000-0000-000000000000",
        "00000000-0000-4000-8000-000000000001",
        "ffffffff-ffff-ffff-ffff-ffffffffffff",
    ] {
        assert_accepted::<fleet_identity_protocol::InstallationId>(json!(uuid));
        assert_accepted::<message_board::ServiceId>(json!(uuid));
    }
    assert_accepted::<message_board::EndpointId>(json!("codex-local"));
    assert_accepted::<message_board::SessionId>(json!("provider\n\t:opaque"));
    for label in [
        "Machine",
        "line\n\ttwo",
        "a\u{007f}b",
        "\r Machine \r",
        "\u{0085}Machine\u{2003}",
    ] {
        assert_accepted::<message_board::MachineLabel>(json!(label));
    }
}
