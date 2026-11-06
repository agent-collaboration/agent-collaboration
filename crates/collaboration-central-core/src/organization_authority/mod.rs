//! Rules owned by an organization's Fleet object.
mod fleet_statement;
mod fleet_statement_refusal;
#[cfg(test)]
mod fleet_statement_test_support;
mod fleet_statement_wire;
mod identity;
mod project_ownership;
mod provisioning;
mod statement_replacement;

pub use fleet_statement::{
    FleetOrganizationStatementTrust, FleetOrganizationStatementVerifier,
    VerifiedFleetOrganizationStatement,
};
pub use fleet_statement_refusal::FleetOrganizationStatementRefusal;
pub use fleet_statement_wire::OrganizationStatementDigest;
pub use identity::{OrganizationIdentity, OrganizationIdentityRefusal, ProvisioningStampAction};
pub use project_ownership::{ProjectOwnershipRefusal, require_project_ownership};
pub use provisioning::{OrganizationProvisioningFailure, OrganizationProvisioningPort};
pub use statement_replacement::{
    OrganizationStatementReplacement, OrganizationStatementReplacementRefusal,
    decide_organization_statement_replacement,
};
