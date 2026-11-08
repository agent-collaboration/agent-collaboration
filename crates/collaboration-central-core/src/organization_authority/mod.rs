//! Rules owned by an organization's Fleet object.
pub(crate) mod change_records;
mod fleet_statement;
mod fleet_statement_refusal;
#[cfg(test)]
mod fleet_statement_test_support;
mod fleet_statement_wire;
mod identity;
mod machine_login;
mod organization_stamp_store;
mod project_ownership;
mod provisioning;
mod sign_in_verification;
mod statement_replacement;

#[cfg(feature = "conformance")]
pub(crate) use change_records::read_page as read_change_record_page;
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
pub use machine_login::{
    AllowanceRow, CloseMachineConnection, Decided, DecisionTime, EnrollmentChange, EnrollmentFacts,
    EnrollmentGeneration, EnrollmentQuery, FormerServiceFacts, ISSUER_CLOCK_SKEW_SECONDS,
    InstallationRow, MachineClaim, MachineEnrollmentFailure, MachineEnrollmentPort,
    MachineLoginSettings, MachineRevocation, MachineTokenAdmission, MachineTokenCheckRefusal,
    MachineTokenPolicy, MachineTokenVerifier, RevocationBy, SD8_MILLIS, StoredEnrollment,
    StoredEnrollmentState, VerifiedMachineToken, admit_machine_connection, admit_machine_token,
    decide_device_login, decide_machine_token, renew_machine_token, revoke_machine_enrollment,
};
pub(crate) use organization_stamp_store::read as read_stored_stamp;
#[cfg(feature = "conformance")]
pub(crate) use organization_stamp_store::tamper as tamper_stored_statement;
pub use organization_stamp_store::{StoredOrganizationStamp, StoredStampRead};
pub use project_ownership::{ProjectOwnershipRefusal, require_project_ownership};
pub use provisioning::ProvisioningApplied;
pub(crate) use provisioning::{
    ApplyProvisioning, ProvisioningOutcome, apply as apply_provisioning,
};
pub use provisioning::{OrganizationProvisioningFailure, OrganizationProvisioningPort};
pub use sign_in_verification::{SignInVerification, SignInVerifiers};
pub use statement_replacement::{
    CurrentStamp, OrganizationStatementReplacement, OrganizationStatementReplacementRefusal,
    decide_organization_statement_replacement,
};
