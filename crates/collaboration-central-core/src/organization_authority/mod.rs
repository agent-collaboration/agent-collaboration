//! Rules owned by an organization's Fleet object.
mod change_records;
mod fleet_statement;
mod fleet_statement_refusal;
#[cfg(test)]
mod fleet_statement_test_support;
mod fleet_statement_wire;
mod identity;
mod organization_stamp_store;
mod project_ownership;
mod provisioning;
mod statement_check;
mod statement_replacement;

pub use change_records::{
    ChangeRequestId, OrganizationChangeKind, OrganizationChangeOrigin, OrganizationChangeRecord,
};
pub use fleet_statement::{
    FleetOrganizationStatementTrust, FleetOrganizationStatementVerifier,
    MAXIMUM_STATEMENT_LIFETIME_SECONDS, VerifiedFleetOrganizationStatement,
};
pub use fleet_statement_refusal::FleetOrganizationStatementRefusal;
pub use fleet_statement_wire::OrganizationStatementDigest;
pub use identity::{OrganizationIdentity, OrganizationIdentityRefusal, ProvisioningStampAction};
pub(crate) use organization_stamp_store::read as read_stored_stamp;
pub use organization_stamp_store::{StoredOrganizationStamp, StoredStampRead};
pub use project_ownership::{ProjectOwnershipRefusal, require_project_ownership};
pub use provisioning::ProvisioningApplied;
pub(crate) use provisioning::{
    ApplyProvisioning, ProvisioningOutcome, apply as apply_provisioning,
};
pub use provisioning::{OrganizationProvisioningFailure, OrganizationProvisioningPort};
pub use statement_check::OrganizationStatementCheck;
pub use statement_replacement::{
    CurrentStamp, OrganizationStatementReplacement, OrganizationStatementReplacementRefusal,
    decide_organization_statement_replacement,
};
