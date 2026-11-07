//! Shared scenarios over the one asynchronous application on each runtime.
#![forbid(unsafe_code)]
#[cfg(feature = "client")]
mod admin_import_scenario;
#[cfg(feature = "client")]
pub use admin_import_scenario::AdminImportScenario;
#[cfg(feature = "client")]
mod adr_runtime;
#[cfg(feature = "client")]
pub use adr_runtime::run_adr_round_trip_conformance;
#[cfg(feature = "client")]
mod feed_history_scenario;
#[cfg(feature = "client")]
mod ledger_fixture;
#[cfg(feature = "client")]
pub use ledger_fixture::typed_ledger_fixtures;
#[cfg(feature = "client")]
mod repository_runtime;
#[cfg(feature = "client")]
pub use feed_history_scenario::run_feed_history_conformance;
#[cfg(feature = "client")]
pub use repository_runtime::run_repository_round_trip_conformance;
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
mod fleet_mail_observer_scenario;
#[cfg(feature = "client")]
mod holder_reply_scenarios;
#[cfg(feature = "client")]
mod lease_session_scenarios;
#[cfg(feature = "client")]
mod mail_status_scenarios;
#[cfg(feature = "client")]
pub use fleet_mail_observer_scenario::run_fleet_mail_observer_conformance;
#[cfg(feature = "client")]
mod native_board_session;
#[cfg(feature = "client")]
mod normal_access_scenarios;
#[cfg(feature = "client")]
mod readmission_scenarios;
#[cfg(feature = "client")]
mod revocation_race_scenarios;
#[cfg(feature = "client")]
mod sender_revocation_scenario;
#[cfg(feature = "client")]
pub use sender_revocation_scenario::run_sender_revocation_conformance;
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
    run_readmission_activation_conformance, run_readmission_reservation_conformance,
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
pub use step_fixture::{receipt_fixture, receipt_with_effects_fixture, typed_receipt_fixture};
#[cfg(feature = "client")]
pub use transaction_reopen::ComposedTransactionScenario;
#[cfg(feature = "client")]
mod archive_runtime;
#[cfg(feature = "client")]
pub use archive_runtime::run_archive_report_with_clock;

#[cfg(feature = "client")]
mod mutation_revocation_scenario;
#[cfg(feature = "client")]
pub use mutation_revocation_scenario::run_mutation_revocation_conformance;

#[cfg(feature = "client")]
pub mod workerd_process;
