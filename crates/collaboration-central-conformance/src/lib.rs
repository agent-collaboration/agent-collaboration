//! Shared scenarios over the one asynchronous application on each runtime.
#![forbid(unsafe_code)]
#[cfg(all(feature = "client", debug_assertions))]
mod automation_clock_scenarios;
#[cfg(all(feature = "client", debug_assertions))]
pub use automation_clock_scenarios::{
    AutomationClockPlatform, AutomationRestartCase, prepare_automation_restart,
    resume_automation_restart, run_automation_clock_conformance,
    run_automation_expired_lease_conformance, run_automation_expiry_conformance,
    run_automation_external_conformance, run_automation_limit_conformance,
    run_automation_retention_conformance, run_automation_wake_delivery_conformance,
};
#[cfg(debug_assertions)]
mod automation_clock_controls;
#[cfg(debug_assertions)]
pub use automation_clock_controls::*;
#[cfg(all(feature = "client", debug_assertions))]
mod remote_session_create_scenarios;
#[cfg(all(feature = "client", debug_assertions))]
pub use remote_session_create_scenarios::{
    RemoteCreateRestartCase, prepare_remote_create_restart, resume_remote_create_restart,
    run_remote_session_create_conformance,
};
#[cfg(feature = "client")]
mod automation_catalog_scenarios;
#[cfg(feature = "client")]
pub use automation_catalog_scenarios::run_automation_catalog_conformance;
#[cfg(all(feature = "client", debug_assertions))]
mod automation_handover_scenarios;
#[cfg(all(feature = "client", debug_assertions))]
pub use automation_handover_scenarios::{
    AutomationHandoverRestartCase, prepare_automation_handover_restart,
    resume_automation_handover_restart, run_automation_handover_carried_conformance,
    run_automation_handover_conformance, run_automation_handover_external_conformance,
    run_automation_handover_lifecycle_conformance, run_automation_handover_repair_conformance,
    run_automation_handover_validation_conformance, run_automation_handover_wake_conformance,
};
#[cfg(feature = "client")]
mod access_expiry_scenario;
#[cfg(feature = "client")]
mod access_fixture;
#[cfg(feature = "client")]
mod access_scenarios;
#[cfg(feature = "client")]
mod composed_client;
#[cfg(feature = "client")]
mod composed_copy_fixture;
#[cfg(feature = "client")]
mod composed_copy_suite;
#[cfg(feature = "client")]
mod composed_fleet_conflict;
#[cfg(feature = "client")]
mod composed_numeric;
#[cfg(feature = "client")]
mod conformance_client;
#[cfg(feature = "client")]
mod enrollment_scenarios;
#[cfg(feature = "client")]
mod holder_reply_scenarios;
#[cfg(feature = "client")]
mod lease_session_scenarios;
#[cfg(feature = "client")]
mod mail_status_scenarios;
#[cfg(feature = "client")]
mod native_board_session;
#[cfg(feature = "client")]
mod normal_access_scenarios;
#[cfg(feature = "client")]
mod readmission_scenarios;
#[cfg(feature = "client")]
mod revocation_race_scenarios;
#[cfg(feature = "client")]
mod runtime_control_scenarios;
mod runtime_controls;
mod runtime_faults;
#[cfg(feature = "client")]
mod scenario_assertions;
#[cfg(feature = "client")]
mod session_replacement_scenarios;
#[cfg(feature = "client")]
mod snapshot_merge;
#[cfg(feature = "client")]
mod staging_access;
#[cfg(feature = "client")]
mod step_fixture;
#[cfg(feature = "client")]
mod transaction_reopen;
#[cfg(feature = "client")]
pub use access_expiry_scenario::{assertion_expiry, run_staging_expiry_row};
#[cfg(feature = "client")]
pub use access_fixture::*;
#[cfg(feature = "client")]
pub use access_scenarios::{
    run_access_expiry_conformance, run_access_key_profile_conformance,
    run_access_verifier_control_conformance, run_access_zero_age_conformance,
};
pub use collaboration_central_core::{AccessCallerClass, AccessVerificationOutcome};
#[cfg(feature = "client")]
pub use composed_copy_suite::run_composed_copy_suite;
#[cfg(feature = "client")]
pub use composed_fleet_conflict::run_composed_fleet_conflict;
#[cfg(feature = "client")]
pub use composed_numeric::ComposedNumericScenario;
#[cfg(feature = "client")]
pub use conformance_client::{ConformanceClient, ConformanceResult};
#[cfg(feature = "client")]
pub use enrollment_scenarios::{
    run_admitted_session_conformance, run_enrollment_identity_conformance,
};
#[cfg(feature = "client")]
pub use holder_reply_scenarios::{
    AdmittedPeerSocket, enroll_admitted_peer, enroll_admitted_peer_with_label,
    enroll_open_peer_with_label, run_holder_reply_conformance,
};
#[cfg(feature = "client")]
pub use lease_session_scenarios::run_lease_renewal_conformance;
#[cfg(feature = "client")]
pub use mail_status_scenarios::run_empty_status_stream_conformance;
#[cfg(feature = "client")]
pub use normal_access_scenarios::run_shared_access_conformance;
#[cfg(feature = "client")]
pub use readmission_scenarios::{
    run_readmission_activation_conformance, run_readmission_open_conformance,
};
#[cfg(feature = "client")]
pub use revocation_race_scenarios::run_revocation_race_conformance;
#[cfg(feature = "client")]
pub use runtime_control_scenarios::run_runtime_control_conformance;
pub use runtime_controls::*;
pub use runtime_faults::{RuntimeFaults, SessionFaultControl, SessionFaultObservation};
#[cfg(feature = "client")]
pub use session_replacement_scenarios::run_session_replacement_conformance;
#[cfg(feature = "client")]
pub use snapshot_merge::run_fleet_snapshot_merge_conformance;
#[cfg(feature = "client")]
pub use staging_access::{StagingAccess, StagingInstallation};
#[cfg(feature = "client")]
pub use step_fixture::{receipt_fixture, receipt_with_effects_fixture};
#[cfg(feature = "client")]
pub use transaction_reopen::ComposedTransactionScenario;
#[cfg(feature = "client")]
mod archive_runtime;
#[cfg(feature = "client")]
pub use archive_runtime::run_archive_report_with_clock;
