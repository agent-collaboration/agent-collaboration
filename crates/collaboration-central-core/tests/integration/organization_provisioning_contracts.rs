//! F1c's closed provisioning payload survives the B2 storage cutover.
use super::contract_artifacts;
use collaboration_central_core::identity_contract::{
    ProvisionOrganization, ProvisionOrganizationResult,
};
use serde_json::json;

// Oracle: B2's provisioning flow and F1c's reviewed literal contract.
// Catch an invented person, a retired operation or an unknown result being accepted.
// No pre-fix semantic red has run for this newly adopted contract on the D1 base.
#[test]
fn provision_organization_wire_contract() {
    contract_artifacts::assert_contract::<ProvisionOrganization>(
        "ProvisionOrganization",
        vec![json!({"organizationId":"019a3dce-2000-7000-8000-000000000001"})],
        false,
    );
}

#[test]
fn provision_organization_result_wire_contract() {
    contract_artifacts::assert_contract::<ProvisionOrganizationResult>(
        "ProvisionOrganizationResult",
        vec![json!({"result":"provisioned"})],
        false,
    );
}
