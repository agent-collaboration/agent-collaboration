//! P8: Welcome refresh lets a never-looked-up installation receive queued offline mail.
#![deny(clippy::too_many_lines)]
#[path = "board_e2e/installation_directory_pair.rs"]
mod installation_directory_pair;
#[allow(dead_code)] // Reuse the process owner; companion board cases use its CLI methods.
#[path = "board_e2e/owned_host.rs"]
mod owned_host;
use codex_router_test_support::{FixtureResult, scenario_finalization::run_scenario};
use installation_directory_pair::DirectoryPair;

#[tokio::test]
async fn welcome_refresh_queues_a_never_looked_up_recipient_after_central_stops() -> FixtureResult {
    run_scenario(
        DirectoryPair::new()?,
        async |pair| {
            pair.start().await?;
            pair.observe_welcome_cache().await?;
            pair.stop_central().await?;
            pair.assert_offline_send().await
        },
        DirectoryPair::close,
        || Ok(()),
    )
    .await
}
