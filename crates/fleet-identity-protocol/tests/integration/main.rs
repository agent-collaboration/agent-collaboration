//! Original test roots retain their case namespaces.

#[cfg(test)]
#[path = "../support/contract_artifacts.rs"]
mod contract_artifacts;
#[cfg(test)]
#[path = "../support/schema_validation.rs"]
mod schema_validation;

mod activity_contracts;
mod connection_wire_contract;
mod identity_value_contracts;
mod registration_contracts;
mod remote_start_contracts;
mod validated_launch_values;
