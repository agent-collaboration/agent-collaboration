#[cfg(test)]
#[path = "support/schema_validation.rs"]
mod schema_validation;

use collaboration_central_core::identity_contract::{MachinePrincipal, MachineTokenClaims};
use serde_json::json;

#[test]
fn full_issuer_payload_keeps_registered_claims_outside_extension_ownership() {
    let payload = json!({
        "iss":"https://issuer.example.invalid", "sub":"owner-account", "aud":"https://central.example.invalid/machine",
        "exp":1793448600, "iat":1793448000, "jti":"019a3dce-2000-7000-8000-000000000001", "client_id":"019a3dce-2000-7000-8000-000000000002",
        "principal":{"kind":"machine","installation_id":"019a3dce-2000-7000-8000-000000000001","enrollment_id":"019a3dce-2000-7000-8000-000000000002"},
        "organization_id":"019a3dce-2000-7000-8000-000000000001", "grant_expires_at":1793448600
    });
    let claims: MachineTokenClaims = serde_json::from_value(payload.clone()).unwrap();
    let schema = serde_json::to_value(schemars::schema_for!(MachineTokenClaims)).unwrap();
    let validator = schema_validation::contract_validator(&schema).unwrap();
    assert!(validator.is_valid(&payload));
    assert_eq!(
        serde_json::to_value(claims).unwrap()["grant_expires_at"],
        json!(1793448600)
    );
    for kind in ["session", "person", "service", "fourthKind"] {
        let mut invalid = payload.clone();
        invalid["principal"]["kind"] = json!(kind);
        assert!(serde_json::from_value::<MachineTokenClaims>(invalid.clone()).is_err());
        assert!(!validator.is_valid(&invalid));
    }
    let mut missing = payload;
    missing.as_object_mut().unwrap().remove("principal");
    assert!(serde_json::from_value::<MachineTokenClaims>(missing.clone()).is_err());
    assert!(!validator.is_valid(&missing));
}

#[test]
fn machine_claims_refuse_camel_case_identity_fields() {
    let invalid = json!({"kind":"machine","installationId":"019a3dce-2000-7000-8000-000000000001","enrollmentId":"019a3dce-2000-7000-8000-000000000002"});
    assert!(serde_json::from_value::<MachinePrincipal>(invalid.clone()).is_err());
    let schema = serde_json::to_value(schemars::schema_for!(MachinePrincipal)).unwrap();
    assert!(
        !schema_validation::contract_validator(&schema)
            .unwrap()
            .is_valid(&invalid)
    );
}
