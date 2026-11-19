//! One asynchronous application core shared by every runtime.
#![forbid(unsafe_code)]

mod engine;
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

mod central_organization_identity;
mod central_timers;
mod guarded_statements;
mod mail_wiring;
pub use central_organization_identity::{
    CentralOrganizationIdentity, prepare_central_alarm, read_central_organization,
};

// Sign-in authority on the org-scoped statement runtime.
mod issuer_key_cache;
mod issuer_key_resolution;
mod issuer_signature_verifier;
pub mod organization_authority;
pub mod person_request;
pub use issuer_key_cache::{IssuerKeyCache, IssuerKeyId, IssuerKeySet, IssuerVerificationKey};
pub use issuer_key_resolution::resolve_issuer_key;
pub use issuer_signature_verifier::IssuerSignatureVerifier;
mod sign_in_trust_settings;
pub use sign_in_trust_settings::SignInTrustSettings;
mod authorization_floor_rows;
mod authorization_revision_plans;
mod organization_admin_contract;
mod organization_admin_plans;
mod organization_authority_cache;
mod organization_change_rows;
mod organization_engine_admin_plans;
mod organization_provisioning_plans;
mod organization_signin_http;
mod organization_signin_port;
mod organization_stamp_rows;
mod person_admission_plans;
pub use organization_admin_contract::*;
pub use organization_signin_http::organization_signin_router;
pub use organization_signin_port::OrganizationSignInPort;
