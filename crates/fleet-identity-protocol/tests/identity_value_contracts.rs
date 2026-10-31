//! Literal wire oracles and serializer-generated artifact freshness.
use fleet_identity_protocol::{
    AbsolutePath, CommitId, EnrollmentId, InstallationId, IsolationInstanceId, ModelName,
    OrganizationId, RemoteStartId, RepositoryLocator, SequenceNumber, UnixMillis, UnixSeconds,
};
use serde_json::json;
#[cfg(test)]
#[path = "support/contract_artifacts.rs"]
mod contract_artifacts;

#[test]
fn borrowed_machine_label_wire_contract() {
    contract_artifacts::assert_contract::<message_board::MachineLabel>(
        "MachineLabel",
        vec![json!("fixture machine"), json!("é".repeat(128))],
        false,
    );
}

#[test]
fn borrowed_session_reference_wire_contract() {
    contract_artifacts::assert_contract::<message_board::SessionRef>(
        "SessionRef",
        vec![
            json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"native/provider:conversation"}),
            json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"é".repeat(2048)}),
        ],
        false,
    );
}

#[test]
fn sequence_number_wire_contract() {
    contract_artifacts::assert_contract::<SequenceNumber>(
        "SequenceNumber",
        vec![json!(0_u64), json!(9007199254740991_u64)],
        false,
    );
}

#[test]
fn organization_id_wire_contract() {
    contract_artifacts::assert_contract::<OrganizationId>(
        "OrganizationId",
        vec![json!("019a3dce-2000-7000-8000-000000000001")],
        false,
    );
}

#[test]
fn enrollment_id_wire_contract() {
    contract_artifacts::assert_contract::<EnrollmentId>(
        "EnrollmentId",
        vec![json!("019a3dce-2000-7000-8000-000000000001")],
        false,
    );
}

#[test]
fn remote_start_id_wire_contract() {
    contract_artifacts::assert_contract::<RemoteStartId>(
        "RemoteStartId",
        vec![json!("019a3dce-2000-7000-8000-000000000001")],
        false,
    );
}

#[test]
fn isolation_instance_id_wire_contract() {
    contract_artifacts::assert_contract::<IsolationInstanceId>(
        "IsolationInstanceId",
        vec![json!("019a3dce-2000-7000-8000-000000000001")],
        false,
    );
}

#[test]
fn installation_id_wire_contract() {
    contract_artifacts::assert_contract::<InstallationId>(
        "InstallationId",
        vec![json!("019a3dce-2000-7000-8000-000000000001")],
        false,
    );
}

#[test]
fn absolute_path_wire_contract() {
    contract_artifacts::assert_contract::<AbsolutePath>(
        "AbsolutePath",
        vec![
            json!("/"),
            json!("/work/item-two"),
            json!(format!("/{}", "a".repeat(4095))),
            json!(format!("/{}a", "é".repeat(2047))),
        ],
        false,
    );
}

#[test]
fn repository_locator_wire_contract() {
    contract_artifacts::assert_contract::<RepositoryLocator>(
        "RepositoryLocator",
        vec![
            json!("https://example.invalid/repository.git"),
            json!("r".repeat(2048)),
            json!("é".repeat(1024)),
        ],
        false,
    );
}

#[test]
fn commit_id_wire_contract() {
    contract_artifacts::assert_contract::<CommitId>(
        "CommitId",
        vec![
            json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            json!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        ],
        false,
    );
}

#[test]
fn model_name_wire_contract() {
    contract_artifacts::assert_contract::<ModelName>(
        "ModelName",
        vec![
            json!("configured-model"),
            json!("m".repeat(256)),
            json!("é".repeat(128)),
        ],
        false,
    );
}

#[test]
fn unix_seconds_wire_contract() {
    contract_artifacts::assert_contract::<UnixSeconds>(
        "UnixSeconds",
        vec![json!(1793448000_i64)],
        false,
    );
}

#[test]
fn unix_millis_wire_contract() {
    contract_artifacts::assert_contract::<UnixMillis>(
        "UnixMillis",
        vec![json!(1793448000000_i64)],
        false,
    );
}
