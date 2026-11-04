//! Real Central publication and cutover interactions.
use board_staging_cutover::{OfflineHost, OfflineHostError, SelectedWriterFile};
mod answer_loss_edge;
mod claim_session;
#[path = "../../src/cutover_host_fixture.rs"]
#[expect(
    dead_code,
    reason = "The shared Host identity fixture also supports unit-only identity refusal cases."
)]
mod cutover_host_fixture;
mod inherited_snapshot_position;
#[path = "../../src/native_snapshot_fixture.rs"]
mod native_snapshot_fixture;
mod publication;
mod publication_replay;
