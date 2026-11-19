//! One asynchronous application core shared by every runtime.
#![forbid(unsafe_code)]

mod clock_ids;
mod engine;
#[cfg(feature = "test-support")]
pub mod engine_owed_probe;
#[cfg(feature = "test-support")]
pub mod engine_probe;
mod engine_wiring;
#[cfg(feature = "test-support")]
pub use engine::DueUnitStart;
#[cfg(feature = "test-support")]
pub use engine::commit_fault::{EngineCommitFault, EngineCommitFaultKind, EngineReadFault};
pub use engine::{AlarmNotPublished, EventCompletion};
pub use engine::{DueBatchSnapshot, DueBatchState, engine_alarm_decider};
pub mod identity_contract;
#[cfg(feature = "test-support")]
pub mod signin_test_observation;
#[cfg(feature = "test-support")]
pub mod storage_conformance;
pub use clock_ids::ClockIds;
mod workspace_turns;
pub use workspace_turns::{WorkspaceTurn, WorkspaceTurns};
#[cfg(feature = "test-support")]
mod pause_points;
#[cfg(feature = "test-support")]
pub use pause_points::{HeldPause, PausePoint, PausePoints};

mod object_host;
pub use object_host::ObjectHost;
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
#[cfg(feature = "test-support")]
pub use workspace_runtime::LegacyUnitInvocations;
pub use workspace_runtime::{WorkspaceCaller, WorkspaceRuntime, WorkspaceStoreCalls};
mod organization_workspace_plans;
mod workspace_actor_identity;
mod workspace_admission;
mod workspace_change_records;
mod workspace_decode_error;
mod workspace_directory_rows;
mod workspace_item_reads;
mod workspace_json_records;
pub mod workspace_migrations;
mod workspace_read_queries;
mod workspace_request_gate;
mod workspace_request_log;
mod workspace_row_codec;
mod workspace_rule_dispatch;
mod workspace_rule_inputs;

mod access_observation;
pub use access_observation::CentralObservation;

mod central_mail_requests;
mod central_mail_rows;
mod central_mail_store;
mod central_timer_plans;
mod central_timer_rows;
mod central_timers;
mod guarded_statements;
pub use engine::AlarmResult;
mod notification_windows;
mod workspace_drain;
mod workspace_notice_plans;
pub use workspace_notice_plans::DrainResult;
mod central_mail_records;
mod central_organization_identity;
pub use central_organization_identity::{
    CentralOrganizationIdentity, prepare_central_alarm, read_central_organization,
};
mod central_mail_time;
pub use central_mail_time::MAX_ALARM_UNIX_MILLIS;

// Sign-in authority on the org-scoped statement runtime.
mod issuer_key_cache;
mod issuer_key_resolution;
mod issuer_signature_verifier;
pub mod organization_authority;
pub mod person_request;
mod signature_verification;
pub use issuer_key_cache::{
    IssuerKeyCache, IssuerKeyId, IssuerKeyRefusal, IssuerKeySet, IssuerVerificationKey,
};
pub use issuer_key_resolution::{IssuerKeyResolutionRefusal, IssuerKeySource, resolve_issuer_key};
pub use issuer_signature_verifier::{IssuerSignatureRefusal, IssuerSignatureVerifier};
pub use signature_verification::{Ed25519Verification, SignatureValidity, VerifyFailure};
mod key_source_capabilities;
pub use key_source_capabilities::{KeyFetchFailure, RawJwks};
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
