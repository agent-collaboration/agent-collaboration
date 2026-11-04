//! The compiled staging helper's public process boundary.
use board_staging_cutover::{OfflineHost, OfflineHostError, SelectedWriterFile};
mod canary_proof;
#[path = "../../src/cutover_host_fixture.rs"]
mod cutover_host_fixture;
mod host_entrypoints;
mod host_operator;
mod host_process;
mod inspect_cli;
mod installed_claim;
mod intent_recovery_proof;
#[path = "../../src/native_snapshot_fixture.rs"]
mod native_snapshot_fixture;
mod offline_cli;
mod offline_cli_fixture;
#[path = "../../src/pending_intent_fixture.rs"]
mod pending_intent_fixture;
mod public_claim;
mod publication_hold;
mod recovery_rows;
