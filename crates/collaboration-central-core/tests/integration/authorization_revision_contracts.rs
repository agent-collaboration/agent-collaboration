//! The fence push central accepts from the sign-in service (r3 §5), published
//! as JSON schema and fixtures for the sign-in side to vendor.
use super::contract_artifacts;
use collaboration_central_core::identity_contract::{
    AuthorizationRevision, AuthorizationRevisionAcknowledgement,
};
use serde_json::json;

// Oracle: the Lead-confirmed push wire (organization row before person rows;
// tagged bodies; 200 {acknowledgedRevision}). Catch an untagged body, an
// unknown state, an out-of-range revision or a bad digest being accepted.
#[test]
fn authorization_revision_wire_contract() {
    contract_artifacts::assert_contract::<AuthorizationRevision>(
        "AuthorizationRevision",
        vec![
            json!({"kind":"organization","organizationId":"019a3dce-2000-7000-8000-000000000001",
                "revision":3,"state":"active",
                "permissionPolicyDigest":"2d31b5d72716e64e255d8858c231f593155289fb03ed354c4b762890667a3c22"}),
            json!({"kind":"person","organizationId":"019a3dce-2000-7000-8000-000000000001",
                "personId":"019a3dce-2000-7000-8000-000000000002",
                "subjectId":"019a3dce-2000-7000-8000-000000000003","revision":4,"state":"inactive"}),
        ],
        false,
    );
}

#[test]
fn authorization_revision_acknowledgement_wire_contract() {
    contract_artifacts::assert_contract::<AuthorizationRevisionAcknowledgement>(
        "AuthorizationRevisionAcknowledgement",
        vec![json!({"acknowledgedRevision":4})],
        false,
    );
}
