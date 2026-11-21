//! Original test roots retain their case namespaces.

#[cfg(test)]
#[path = "../support/contract_artifacts.rs"]
mod contract_artifacts;
#[cfg(test)]
#[path = "../support/schema_validation.rs"]
mod schema_validation;

mod enrollment_contracts;
mod identity_boundary_rules;
mod issuer_decision_contracts;
mod machine_claim_boundaries;
mod machine_login_mirror_vectors;
mod organization_admin_contracts;
mod organization_command_contracts;
