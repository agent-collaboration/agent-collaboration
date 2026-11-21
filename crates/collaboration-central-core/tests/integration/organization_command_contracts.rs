#[cfg(test)]
use crate::contract_artifacts;
use collaboration_central_core::identity_contract::AllowanceState;
use serde_json::json;

#[test]
fn revoke_permission_wire_contract() {
    use collaboration_central_core::identity_contract::RevokePermission;

    contract_artifacts::assert_contract::<RevokePermission>(
        "RevokePermission",
        vec![json!("machine:revoke")],
        false,
    );
}

#[test]
fn allowance_state_wire_contract() {
    contract_artifacts::assert_contract::<AllowanceState>(
        "AllowanceState",
        vec![json!("active"), json!("withdrawn")],
        false,
    );
}
