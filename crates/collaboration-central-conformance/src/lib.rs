//! **Superseded structure (2026-10-16).** The scenarios here carry over. What goes is the double
//! launch plumbing and the duplicated control dispatch, which exist because each runtime had its
//! own application of central. The per-runtime fault seams stay, as one test wrapper around each
//! runtime's real capability, and the same scenarios run against the one core on each runtime.
//! See `docs/specs/2026-10-16-central-one-core/program-design.md`, section 13.
#![forbid(unsafe_code)]
#[cfg(feature = "client")]
mod access_fixture;
#[cfg(feature = "client")]
mod call_edge_scenarios;
mod control_contract;
#[cfg(feature = "client")]
pub use call_edge_scenarios::run_call_edge_conformance;
#[cfg(feature = "client")]
mod import_fixtures;
#[cfg(feature = "client")]
mod import_restart;
#[cfg(feature = "client")]
pub use import_restart::ImportClaimRestartScenario;
#[cfg(feature = "client")]
mod archive_runtime;
#[cfg(feature = "client")]
mod directory_restart;
#[cfg(feature = "client")]
mod fleet_directory_fixture;
#[cfg(feature = "client")]
mod registry_rpc_driver;
#[cfg(feature = "client")]
pub use archive_runtime::run_archive_report_conformance;
#[cfg(feature = "client")]
mod lease_order_restart;
#[cfg(feature = "client")]
pub use lease_order_restart::LeaseOrderRestartScenario;
#[cfg(feature = "client")]
mod archive_cleanup_restart;
#[cfg(feature = "client")]
pub use archive_cleanup_restart::{ArchiveCleanupRestartScenario, CleanupLoss};
#[cfg(feature = "client")]
pub use directory_restart::ImportDirectoryRestartScenario;
#[cfg(feature = "client")]
mod import_scenarios;
#[cfg(feature = "client")]
mod import_validation;
#[cfg(feature = "client")]
pub use import_scenarios::{run_import_claim_conformance, run_import_claim_roundtrip};
#[cfg(feature = "client")]
mod forward_scenarios;
#[cfg(feature = "client")]
pub use access_fixture::*;
#[cfg(feature = "client")]
pub use forward_scenarios::{
    ForwardRestartScenario, run_forward_conformance, run_forward_roundtrip_and_race,
};
#[cfg(feature = "client")]
mod access_scenarios;
#[cfg(feature = "client")]
pub use access_scenarios::{run_access_conformance, run_access_verifier_control_conformance};
pub use control_contract::{
    AccessCallerClass, AccessVerificationOutcome, CallEdgeProbe, ControlRequest, ControlResult,
    StorageCorruption,
};
#[cfg(feature = "client")]
mod conformance_client;
#[cfg(feature = "client")]
mod copy_extended_scenarios;
#[cfg(feature = "client")]
mod copy_scenarios;
#[cfg(feature = "client")]
mod holder_fixtures;
#[cfg(feature = "client")]
mod session_resume;
#[cfg(feature = "client")]
mod staging_access;
#[cfg(feature = "client")]
pub use staging_access::{StagingAccess, StagingInstallation, owner_app_token};
#[cfg(feature = "client")]
mod staging_access_rows;
#[cfg(feature = "client")]
pub use staging_access_rows::{ProbeObservation, guard_normal_target, run_staging_access_rows};
#[cfg(feature = "client")]
mod staging_normal_rows;
#[cfg(feature = "client")]
mod staging_transport_rows;
#[cfg(feature = "client")]
pub use staging_normal_rows::{
    NormalRedeployState, prepare_staging_normal_rows, run_staging_hibernation_rows,
    run_staging_renewal_rows,
};
#[cfg(feature = "client")]
pub use staging_transport_rows::run_staging_transport_rows;
#[cfg(feature = "client")]
mod staging_scenarios;
#[cfg(feature = "client")]
pub use staging_scenarios::{
    DeploySentinel, assertion_expiry, capture_machine_assertion, guard_fault_target,
    run_staging_expiry_row, run_staging_replacement_row, run_staging_revocation_row,
    run_staging_session_conformance, run_staging_verifier_rows,
};
#[cfg(feature = "client")]
mod ordered_transfer_rows;
#[cfg(feature = "client")]
mod session_scenarios;
#[cfg(feature = "client")]
mod transfer_scenarios;
#[cfg(feature = "client")]
mod unproven_transfer_rows;
#[cfg(feature = "client")]
pub use conformance_client::{ConformanceClient, ConformanceResult, HolderSession};
#[cfg(feature = "client")]
pub use copy_scenarios::run_copy_conformance;
#[cfg(feature = "client")]
pub use session_resume::SessionResumeScenario;
#[cfg(feature = "client")]
pub use session_scenarios::run_session_conformance;
#[cfg(feature = "client")]
pub use transfer_scenarios::held_fence_and_piece_resolve_to_one_complete_prefix;
#[cfg(feature = "client")]
pub use unproven_transfer_rows::run_unproven_transfer_rows;
#[cfg(feature = "client")]
mod numeric_scenarios;
#[cfg(feature = "client")]
pub use numeric_scenarios::{NumericRestartScenario, run_numeric_conformance};
#[cfg(feature = "client")]
mod restart_scenarios;
#[cfg(feature = "client")]
pub use restart_scenarios::{TransactionRestartScenario, TransferRestartScenario};
#[cfg(feature = "client")]
mod default_smoke;
#[cfg(feature = "client")]
mod fleet_feed_restart;
#[cfg(feature = "client")]
mod fleet_outbox_restart;
#[cfg(feature = "client")]
mod notice_runtime;
#[cfg(feature = "client")]
mod rejected_tail_restart;
#[cfg(feature = "client")]
pub use fleet_outbox_restart::{FleetOutboxRestartScenario, NoticeDeliveryLoss};
#[cfg(feature = "client")]
pub use notice_runtime::{NoticeRestartScenario, run_fleet_notice_conformance};
#[cfg(feature = "client")]
pub use rejected_tail_restart::RejectedTailRestartScenario;
#[cfg(feature = "client")]
mod fleet_import_scenarios;
#[cfg(feature = "client")]
pub use fleet_feed_restart::{FleetFeedAfterPruning, FleetFeedRestartScenario};
#[cfg(feature = "client")]
mod fleet_request_scenarios;
#[cfg(feature = "client")]
pub use fleet_request_scenarios::{run_fleet_conflict_conformance, run_fleet_request_conformance};
#[cfg(feature = "client")]
mod malformed_frame_scenarios;
#[cfg(feature = "client")]
pub use default_smoke::run_default_smoke;
#[cfg(feature = "client")]
pub use fleet_import_scenarios::run_fleet_import_conformance;
#[cfg(feature = "client")]
pub use malformed_frame_scenarios::run_malformed_frame_conformance;
#[cfg(feature = "client")]
mod panic_observation;
#[cfg(feature = "client")]
pub use panic_observation::PanicObservation;
