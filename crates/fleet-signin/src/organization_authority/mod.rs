//! Organization stamp verification, identity and replacement rules.
mod fleet_statement;
#[cfg(test)]
mod fleet_statement_test_support;
mod fleet_statement_wire;
mod identity;
mod provisioning_failure;
mod sign_in_verification;
mod statement_replacement;
mod stored_stamp;

pub use crate::organization_change_rows::OrganizationChangeRecord;
pub use fleet_statement::{
    FleetOrganizationStatementTrust, FleetOrganizationStatementVerifier,
    MAXIMUM_STATEMENT_LIFETIME_SECONDS, StoredStatementFacts, VerifiedFleetOrganizationStatement,
};
pub use fleet_statement_wire::{OrganizationStatementDigest, stored_statement_unexpired};
pub use identity::{OrganizationIdentity, OrganizationIdentityRefusal, ProvisioningStampAction};
pub use provisioning_failure::OrganizationProvisioningFailure;
pub use sign_in_verification::{SignInVerification, SignInVerifiers};
pub use statement_replacement::{
    CurrentStamp, OrganizationStatementReplacement, OrganizationStatementReplacementRefusal,
    decide_organization_statement_replacement,
};
pub use stored_stamp::{StoredOrganizationStamp, StoredStampRead};
