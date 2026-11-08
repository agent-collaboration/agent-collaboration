//! Original test roots retain their case namespaces.

#[cfg(test)]
#[path = "../support/contract_artifacts.rs"]
mod contract_artifacts;
#[cfg(test)]
#[path = "../support/schema_validation.rs"]
mod schema_validation;

mod access_rules;
mod admin_lookup_result_contracts;
mod authorization_revision_contracts;
mod call_edge_mapping;
mod enrollment_contracts;
mod identity_boundary_rules;
mod issuer_decision_contracts;
mod issuer_policy;
mod machine_claim_boundaries;
mod object_addresses;
mod organization_command_contracts;
mod organization_provisioning_contracts;
mod presence_lookup_contracts;
