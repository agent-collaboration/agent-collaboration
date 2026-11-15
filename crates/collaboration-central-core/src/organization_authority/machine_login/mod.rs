//! Pure organization-owned machine-login decisions and guarded change plans.
mod decision_support;
mod device_login_decision;
mod enrollment_records;
mod machine_token_decision;
#[cfg(any(test, feature = "test-support"))]
mod revoke_enrollment;

pub(crate) use decision_support::{DecisionTime, IDLE_EXPIRY_SECONDS};
pub(crate) use device_login_decision::decide_device_login;
pub(crate) use enrollment_records::*;
pub(crate) use machine_token_decision::decide_machine_token;
#[cfg(feature = "test-support")]
pub(crate) use revoke_enrollment::revoke_machine_enrollment;

#[cfg(test)]
mod policy_fixture;
