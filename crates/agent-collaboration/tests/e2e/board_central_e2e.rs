//! BC01–BC04 and BC06: approved local central journeys through compiled CLI and real Hosts.

#![deny(clippy::too_many_lines)]
#[path = "../board_e2e/central_archive.rs"]
mod central_archive;
#[path = "../board_e2e/central_forwarding.rs"]
mod central_forwarding;
#[path = "../board_e2e/central_holders.rs"]
mod central_holders;
use crate::central_pair;
#[path = "../board_e2e/central_replication.rs"]
mod central_replication;
use crate::fixture;
use crate::owned_host;
use crate::records_reopen;
use codex_router_test_support::FixtureResult;

#[tokio::test]
async fn bc01_replicated_prefix_mapping_replay_and_owning_reopen() -> FixtureResult {
    central_pair::scenario(central_replication::replication).await
}

#[tokio::test]
async fn bc02_move_and_takeover_install_exact_native_authority() -> FixtureResult {
    central_pair::scenario(central_holders::holder_change).await
}

#[tokio::test]
async fn bc03_archive_notices_settle_and_copied_content_stays_readable() -> FixtureResult {
    central_pair::scenario(central_archive::archive).await
}

#[tokio::test]
async fn bc04_forwarding_uses_verified_origin_and_replays_one_effect() -> FixtureResult {
    central_pair::scenario(central_forwarding::verified_origin).await
}

#[tokio::test]
async fn bc06_committed_lease_expiry_changes_no_version_or_reader_copy() -> FixtureResult {
    central_pair::scenario(central_holders::directory_expiry).await
}
