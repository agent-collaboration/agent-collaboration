//! Literal wire oracles and serializer-generated artifact freshness.
use collaboration_central_core::identity_contract::{
    AdmissionId, AdmissionRef, ClientId, CommandId, DeviceLoginRequestId, EnrollmentAuthorization,
    EnrollmentRevocationReason, MachineEnrollment, PersonId,
};
use serde_json::json;
#[cfg(test)]
#[path = "support/contract_artifacts.rs"]
mod contract_artifacts;

#[test]
fn admission_ref_wire_contract() {
    contract_artifacts::assert_contract::<AdmissionRef>(
        "AdmissionRef",
        vec![
            json!({"kind":"admission","admissionId":"019a3dce-2000-7000-8000-000000000001"}),
            json!({"kind":"machine","installation":"019a3dce-2000-7000-8000-000000000001"}),
        ],
        false,
    );
}

#[test]
fn enrollment_authorization_wire_contract() {
    contract_artifacts::assert_contract::<EnrollmentAuthorization>(
        "EnrollmentAuthorization",
        vec![
            json!({"kind":"deviceCodeApproval","personId":"019a3dce-2000-7000-8000-000000000001","requestId":"019a3dce-2000-7000-8000-000000000002"}),
        ],
        false,
    );
}

#[test]
fn machine_enrollment_wire_contract() {
    contract_artifacts::assert_contract::<MachineEnrollment>(
        "MachineEnrollment",
        vec![
            json!({"state":"enrolled","enrollmentId":"019a3dce-2000-7000-8000-000000000002","installation":"019a3dce-2000-7000-8000-000000000001","authorizedBy":{"kind":"deviceCodeApproval","personId":"019a3dce-2000-7000-8000-000000000001","requestId":"019a3dce-2000-7000-8000-000000000002"},"machineClientId":"019a3dce-2000-7000-8000-000000000002"}),
            json!({"state":"enrolled","enrollmentId":"019a3dce-2000-7000-8000-000000000002","installation":"019a3dce-2000-7000-8000-000000000001","authorizedBy":{"kind":"deviceCodeApproval","personId":"019a3dce-2000-7000-8000-000000000001","requestId":"019a3dce-2000-7000-8000-000000000002"},"machineClientId":"019a3dce-2000-7000-8000-000000000002","endsAt":1793448000_i64}),
            json!({"state":"revoked","enrollmentId":"019a3dce-2000-7000-8000-000000000002","installation":"019a3dce-2000-7000-8000-000000000001","authorizedBy":{"kind":"deviceCodeApproval","personId":"019a3dce-2000-7000-8000-000000000001","requestId":"019a3dce-2000-7000-8000-000000000002"},"machineClientId":"019a3dce-2000-7000-8000-000000000002","reason":{"kind":"owner"}}),
            json!({"state":"revoked","enrollmentId":"019a3dce-2000-7000-8000-000000000002","installation":"019a3dce-2000-7000-8000-000000000001","authorizedBy":{"kind":"deviceCodeApproval","personId":"019a3dce-2000-7000-8000-000000000001","requestId":"019a3dce-2000-7000-8000-000000000002"},"machineClientId":"019a3dce-2000-7000-8000-000000000002","endsAt":1793448000_i64,"reason":{"kind":"owner"}}),
            json!({"state":"expired","enrollmentId":"019a3dce-2000-7000-8000-000000000002","installation":"019a3dce-2000-7000-8000-000000000001","authorizedBy":{"kind":"deviceCodeApproval","personId":"019a3dce-2000-7000-8000-000000000001","requestId":"019a3dce-2000-7000-8000-000000000002"},"machineClientId":"019a3dce-2000-7000-8000-000000000002"}),
            json!({"state":"expired","enrollmentId":"019a3dce-2000-7000-8000-000000000002","installation":"019a3dce-2000-7000-8000-000000000001","authorizedBy":{"kind":"deviceCodeApproval","personId":"019a3dce-2000-7000-8000-000000000001","requestId":"019a3dce-2000-7000-8000-000000000002"},"machineClientId":"019a3dce-2000-7000-8000-000000000002","endsAt":1793448000_i64}),
            json!({"state":"revoked","enrollmentId":"019a3dce-2000-7000-8000-000000000002","installation":"019a3dce-2000-7000-8000-000000000001","authorizedBy":{"kind":"deviceCodeApproval","personId":"019a3dce-2000-7000-8000-000000000001","requestId":"019a3dce-2000-7000-8000-000000000002"},"machineClientId":"019a3dce-2000-7000-8000-000000000002","reason":{"kind":"superseded"}}),
        ],
        false,
    );
}

#[test]
fn enrollment_revocation_reason_wire_contract() {
    contract_artifacts::assert_contract::<EnrollmentRevocationReason>(
        "EnrollmentRevocationReason",
        vec![json!({"kind":"owner"}), json!({"kind":"superseded"})],
        false,
    );
}

#[test]
fn person_id_wire_contract() {
    contract_artifacts::assert_contract::<PersonId>(
        "PersonId",
        vec![json!("019a3dce-2000-7000-8000-000000000001")],
        false,
    );
}

#[test]
fn client_id_wire_contract() {
    contract_artifacts::assert_contract::<ClientId>(
        "ClientId",
        vec![json!("019a3dce-2000-7000-8000-000000000001")],
        false,
    );
}

#[test]
fn device_login_request_id_wire_contract() {
    contract_artifacts::assert_contract::<DeviceLoginRequestId>(
        "DeviceLoginRequestId",
        vec![json!("019a3dce-2000-7000-8000-000000000001")],
        false,
    );
}

#[test]
fn admission_id_wire_contract() {
    contract_artifacts::assert_contract::<AdmissionId>(
        "AdmissionId",
        vec![json!("019a3dce-2000-7000-8000-000000000001")],
        false,
    );
}

#[test]
fn command_id_wire_contract() {
    contract_artifacts::assert_contract::<CommandId>(
        "CommandId",
        vec![json!("019a3dce-2000-7000-8000-000000000001")],
        false,
    );
}
