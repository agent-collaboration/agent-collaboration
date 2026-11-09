//! B3 machine login at the organization authority: the device-login and
//! machine-token decisions, connection admission and revocation. These are
//! pure decisions over `MachineEnrollmentPort`. The host injects the store,
//! the clock and the ID source.
mod decision_support;
mod device_login_decision;
mod enrollment_liveness;
mod enrollment_port;
mod enrollment_records;
mod enrollment_sql_codec;
mod enrollment_sql_store;
#[cfg(test)]
mod in_memory_enrollments;
mod machine_connection;
mod machine_token_check;
mod machine_token_decision;
#[cfg(test)]
mod sql_store_fixture;
#[cfg(test)]
mod test_fixtures;

pub use decision_support::{DecisionTime, MachineLoginSettings};
pub use device_login_decision::{SD8_MILLIS, decide_device_login};
pub use enrollment_port::{
    Decided, EnrollmentFacts, EnrollmentQuery, MachineEnrollmentFailure, MachineEnrollmentPort,
};
pub use enrollment_records::{
    AllowanceRow, EnrollmentChange, EnrollmentGeneration, FormerServiceFacts, InstallationRow,
    MachineClaim, StoredEnrollment, StoredEnrollmentState,
};
pub use enrollment_sql_store::SqlEnrollmentPort;
pub use machine_connection::{
    CloseMachineConnection, MachineRevocation, RevocationBy, admit_machine_connection,
    revoke_machine_enrollment,
};
pub use machine_token_check::{
    ISSUER_CLOCK_SKEW_SECONDS, MachineTokenAdmission, MachineTokenCheckRefusal, MachineTokenPolicy,
    MachineTokenVerifier, VerifiedMachineToken, admit_machine_token, renew_machine_token,
};
pub use machine_token_decision::decide_machine_token;
