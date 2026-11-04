//! The compiled staging helper's public process boundary.
use board_staging_cutover::{OfflineHost, OfflineHostError};
#[path = "../../src/cutover_host_fixture.rs"]
#[expect(
    dead_code,
    reason = "The shared fixture also supports library refusal cases."
)]
mod cutover_host_fixture;
mod inspect_cli;
