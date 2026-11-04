//! The compiled staging helper's public process boundary.
use board_staging_cutover::{OfflineHost, OfflineHostError, SelectedWriterFile};
#[path = "../../src/cutover_host_fixture.rs"]
#[expect(
    dead_code,
    reason = "The shared fixture also supports library refusal cases."
)]
mod cutover_host_fixture;
mod inspect_cli;
#[path = "../../src/native_snapshot_fixture.rs"]
mod native_snapshot_fixture;
mod offline_cli;
mod offline_cli_fixture;
mod publication_hold;
