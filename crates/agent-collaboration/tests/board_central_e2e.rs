//! BC01–BC06: approved local central journeys through compiled CLI and real Hosts.
#![allow(clippy::panic_in_result_fn)]
#![deny(clippy::too_many_lines)]
#[path = "board_e2e/central_archive.rs"]
mod central_archive;
#[path = "board_e2e/central_forwarding.rs"]
mod central_forwarding;
#[path = "board_e2e/central_holders.rs"]
mod central_holders;
#[path = "board_e2e/central_import.rs"]
mod central_import;
#[path = "board_e2e/central_joint.rs"]
mod central_joint;
#[path = "board_e2e/central_pair.rs"]
mod central_pair;
#[path = "board_e2e/central_replication.rs"]
mod central_replication;
#[path = "board_e2e/fixture.rs"]
#[allow(dead_code)] // The single shared fixture has additional board-record consumers.
mod fixture;
#[path = "board_e2e/fixture_inputs.rs"]
#[allow(dead_code)]
mod fixture_inputs;
#[path = "../../board-legacy-upgrade/tests/upload_support/actual_cli.rs"]
mod legacy_publisher;
#[path = "board_e2e/owned_host.rs"]
#[allow(dead_code)] // Crash-only shared APIs belong to the takeover target.
mod owned_host;
#[path = "board_e2e/records_reopen.rs"]
#[allow(dead_code)] // Reuse the owner's stopped native readback, without a second SQL owner.
mod records_reopen;
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
async fn bc05_actual_import_claim_move_takeover_and_forwarding() -> FixtureResult {
    central_pair::router_root_scenario(central_import::joint).await
}

#[tokio::test]
async fn bc06_committed_lease_expiry_changes_no_version_or_reader_copy() -> FixtureResult {
    central_pair::scenario(central_holders::directory_expiry).await
}
