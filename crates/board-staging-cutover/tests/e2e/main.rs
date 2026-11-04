//! The compiled staging helper's public process boundary.
use board_staging_cutover::{OfflineHost, OfflineHostError, SelectedWriterFile};
#[path = "../../src/cutover_host_fixture.rs"]
mod cutover_host_fixture;
mod host_entrypoints;
mod host_operator;
mod host_process;
mod inspect_cli;
mod installed_claim;
#[path = "../../src/native_snapshot_fixture.rs"]
mod native_snapshot_fixture;
mod offline_cli;
mod offline_cli_fixture;
mod publication_hold;
