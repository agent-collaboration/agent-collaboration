//! One asynchronous application core shared by every runtime.
#![forbid(unsafe_code)]

#[cfg(feature = "test-support")]
pub mod engine_owed_probe;
#[cfg(feature = "test-support")]
pub mod engine_probe;
mod engine_wiring;
pub mod identity_contract;
#[cfg(feature = "test-support")]
pub mod signin_test_observation;
#[cfg(feature = "test-support")]
pub mod storage_conformance;

mod machine_login_plans;
mod machine_login_reads;
mod machine_login_row_values;
mod machine_login_rows;
#[cfg(feature = "test-support")]
mod machine_login_test_control;
mod machine_login_writes;
mod workspace_runtime;
#[cfg(feature = "test-support")]
pub use machine_login_test_control::{MachineLoginControl, machine_login_control};
pub use workspace_runtime::WorkspaceRuntime;
mod organization_workspace_plans;
mod workspace_request_gate;

mod central_timers;
mod mail_wiring;

// Sign-in authority on the org-scoped statement runtime.
mod organization_admin_contract;
mod organization_admin_plans;
pub mod organization_authority;
mod organization_engine_admin_plans;
mod organization_signin_http;
mod organization_signin_port;
mod person_admission_plans;
mod signin_alarm_policy;
mod signin_wiring;
pub use organization_admin_contract::*;
pub use organization_signin_http::organization_signin_router;
pub use organization_signin_port::OrganizationSignInPort;
pub use signin_alarm_policy::prepare_central_alarm;
