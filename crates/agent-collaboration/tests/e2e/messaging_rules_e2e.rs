//! B+ process observations compose the real Router entry points with local central.
#![deny(clippy::too_many_lines)]
#[path = "../board_e2e/messaging_capacity.rs"]
mod messaging_capacity;
#[path = "../board_e2e/messaging_expiry.rs"]
mod messaging_expiry;
#[path = "../board_e2e/messaging_fixture.rs"]
mod messaging_fixture;
#[path = "../board_e2e/messaging_recovery.rs"]
mod messaging_recovery;
#[path = "../board_e2e/messaging_replay.rs"]
mod messaging_replay;
#[path = "../board_e2e/messaging_surface.rs"]
mod messaging_surface;
#[path = "../board_e2e/messaging_uncertainty.rs"]
mod messaging_uncertainty;
