#![deny(clippy::too_many_lines)]

#[path = "upgrade_binary_e2e/ib01_preparation.rs"]
mod ib01_preparation;
#[path = "upgrade_binary_e2e/ib01_publication.rs"]
mod ib01_publication;
#[path = "upgrade_binary_e2e/ib02_replay.rs"]
mod ib02_replay;
#[path = "upgrade_binary_e2e/ib03_prepublication_restart.rs"]
mod ib03_prepublication_restart;
#[path = "upgrade_binary_e2e/ib04_postpublication_restart.rs"]
mod ib04_postpublication_restart;
