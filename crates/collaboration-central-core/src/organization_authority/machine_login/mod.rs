//! Pure organization-owned machine-login decisions and guarded change plans.
mod decision_support;
mod device_login_decision;
mod enrollment_records;
mod machine_token_decision;
mod revoke_enrollment;

pub(crate) use decision_support::DecisionTime;
pub use decision_support::MachineLoginSettings;
pub(crate) use device_login_decision::{SD8_MILLIS, decide_device_login};
pub(crate) use enrollment_records::*;
pub(crate) use machine_token_decision::decide_machine_token;
pub(crate) use revoke_enrollment::revoke_machine_enrollment;

#[cfg(test)]
mod policy_fixture;
