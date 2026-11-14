//! Organization stamp trust and pure provisioning/replacement policy.
mod fleet_statement;
mod fleet_statement_refusal;
#[cfg(test)]
mod fleet_statement_test_support;
mod fleet_statement_wire;
mod identity;
mod sign_in_verification;
mod statement_replacement;

pub use crate::organization_change_rows::OrganizationChangeRecord;
mod provisioning_failure;
pub use provisioning_failure::OrganizationProvisioningFailure;
mod stored_stamp;
pub use fleet_statement::{
    FleetOrganizationStatementTrust, FleetOrganizationStatementVerifier,
    MAXIMUM_STATEMENT_LIFETIME_SECONDS, VerifiedFleetOrganizationStatement,
};
pub use fleet_statement_refusal::FleetOrganizationStatementRefusal;
pub use fleet_statement_wire::OrganizationStatementDigest;
pub(crate) use fleet_statement_wire::stored_statement_unexpired;
pub use identity::{OrganizationIdentity, OrganizationIdentityRefusal, ProvisioningStampAction};
pub use sign_in_verification::{SignInVerification, SignInVerifiers};
pub use statement_replacement::{
    CurrentStamp, OrganizationStatementReplacement, OrganizationStatementReplacementRefusal,
    decide_organization_statement_replacement,
};
pub use stored_stamp::{StoredOrganizationStamp, StoredStampRead};
