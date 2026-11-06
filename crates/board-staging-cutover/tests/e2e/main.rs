//! The compiled staging helper's public process boundary.
use board_staging_cutover::{OfflineHost, OfflineHostError, SelectedWriterFile};
mod canary_proof;
#[path = "../integration/claim_session.rs"]
mod claim_session;
mod compiled_canary_proof;
mod cross_machine_canary;
mod custody_fault_fixture;
#[path = "../../src/cutover_host_fixture.rs"]
mod cutover_host_fixture;
mod cutover_preview;
mod direct_central_enrollment;
mod host_entrypoints;
mod host_operator;
mod host_process;
mod inspect_cli;
mod installed_claim;
mod intent_recovery_proof;
mod legacy_bundle_resume;
mod native_epoch_admission;
#[path = "../../src/native_snapshot_fixture.rs"]
mod native_snapshot_fixture;
mod native_source_selection;
mod offline_cli;
mod offline_cli_fixture;
#[path = "../../src/pending_intent_fixture.rs"]
mod pending_intent_fixture;
mod production_journey;
mod production_mode;
mod public_claim;
mod publication_diagnostics;
mod publication_hold;
mod recovery_rows;
mod rich_project_cutover;
mod rich_source_messages;
mod rich_source_readback;
mod rich_source_records;
mod saved_bundle_resume;
mod verification_extra_write;
