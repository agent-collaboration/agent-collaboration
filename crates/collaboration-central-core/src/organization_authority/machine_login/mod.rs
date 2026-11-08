//! B3 machine login at the organization authority: the device-login and
//! machine-token decisions, connection admission and revocation. These are
//! pure decisions over `MachineEnrollmentPort`. The host injects the store,
//! the clock and the ID source.
mod decision_support;
mod device_login_decision;
mod enrollment_liveness;
mod enrollment_port;
mod enrollment_records;
#[cfg(test)]
mod in_memory_enrollments;
mod machine_connection;
mod machine_token_decision;
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
pub use machine_connection::{
    CloseMachineConnection, MachineRevocation, RevocationBy, admit_machine_connection,
    revoke_machine_enrollment,
};
pub use machine_token_decision::decide_machine_token;
