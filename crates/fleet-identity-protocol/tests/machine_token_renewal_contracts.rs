//! The adopted renewal reply has one schema and one closed refusal vocabulary.
use fleet_identity_protocol::{MachineTokenRenewal, MachineTokenRenewalRefusal};
use serde_json::json;
#[cfg(test)]
#[path = "support/contract_artifacts.rs"]
mod contract_artifacts;

#[test]
fn machine_token_renewal_wire_contract() {
    contract_artifacts::assert_contract::<MachineTokenRenewal>(
        "MachineTokenRenewal",
        vec![
            json!({"result":"renewed"}),
            json!({"result":"refused","reason":{"kind":"tokenInvalid"}}),
            json!({"result":"refused","reason":{"kind":"tokenExpired"}}),
            json!({"result":"refused","reason":{"kind":"wrongClient"}}),
            json!({"result":"refused","reason":{"kind":"wrongInstallation"}}),
            json!({"result":"refused","reason":{"kind":"enrollmentNotActive"}}),
        ],
        false,
    );
}

#[test]
fn machine_token_renewal_refusal_wire_contract() {
    contract_artifacts::assert_contract::<MachineTokenRenewalRefusal>(
        "MachineTokenRenewalRefusal",
        vec![
            json!({"kind":"tokenInvalid"}),
            json!({"kind":"tokenExpired"}),
            json!({"kind":"wrongClient"}),
            json!({"kind":"wrongInstallation"}),
            json!({"kind":"enrollmentNotActive"}),
        ],
        false,
    );
}
