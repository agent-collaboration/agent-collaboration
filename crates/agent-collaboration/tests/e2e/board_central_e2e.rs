//! BC01–BC04 and BC06: approved local central journeys through compiled CLI and real Hosts.

#![deny(clippy::too_many_lines)]
#[path = "../board_e2e/central_archive.rs"]
mod central_archive;
#[path = "../board_e2e/central_forwarding.rs"]
mod central_forwarding;
#[path = "../board_e2e/central_holders.rs"]
mod central_holders;
#[path = "../board_e2e/central_replication.rs"]
mod central_replication;
