use codex_router_test_support::installed_codex::run_all_weekly_exhausted_terminal;
use codex_router_test_support::installed_codex::run_three_account_short_quota_reconnect;

#[tokio::test]
#[ignore = "installed Codex integration; run with cargo nextest run --locked --profile default -p codex-router-test-support --test integration -E 'test(/^codex_retry::/)' --run-ignored only --test-threads 1"]
async fn three_account_5h_reset_reconnects_and_completes() -> Result<(), String> {
    run_three_account_short_quota_reconnect().await
}

#[tokio::test]
#[ignore = "installed Codex integration; run with cargo nextest run --locked --profile default -p codex-router-test-support --test integration -E 'test(/^codex_retry::/)' --run-ignored only --test-threads 1"]
async fn all_accounts_weekly_exhausted_stops() -> Result<(), String> {
    run_all_weekly_exhausted_terminal().await
}
