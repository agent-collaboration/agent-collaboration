//! Opt-in real quota screen capture. No production account or native Host is used.

#![cfg(unix)]

#[path = "quota_storyboard_capture_test/mod.rs"]
mod capture_support;
#[path = "../quota_reset/quota_reset_pty_test/isolated_fixture_test.rs"]
#[expect(
    dead_code,
    reason = "Read-only shared fixture also exposes cached-credit seeding, outside this journey"
)]
mod isolated_fixture_test;
#[path = "../quota_reset/quota_reset_pty_test/loopback_provider_test.rs"]
mod loopback_provider_test;

#[path = "quota_storyboard_capture_test/component_first_frame_test.rs"]
mod component_first_frame_test;

#[path = "quota_storyboard_capture_test/grid_terminal_capture.rs"]
mod grid_terminal_capture;
#[path = "../quota_reset/quota_reset_pty_test/terminal_interaction_test.rs"]
#[expect(
    dead_code,
    reason = "Shared PTY owner retains the existing semantic-wait API for its original tests"
)]
mod terminal_interaction_test;

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires a declared grid-observer target and an interactive driver protocol"]
async fn compiled_grid_terminal_observation() -> TestResult<()> {
    grid_terminal_capture::run_grid_terminal().await
}

use isolated_fixture_test::TestResult;

#[path = "quota_storyboard_capture_test/public_sessions_capture.rs"]
mod public_sessions_capture;
#[test]
#[ignore = "requires separately authorized public Host, effects packet and grid observer"]
fn compiled_public_sessions_grid_observation() -> TestResult<()> {
    public_sessions_capture::run_live_public_sessions()
}

#[path = "quota_storyboard_capture_test/public_grid_transport_test.rs"]
mod public_grid_transport_test;

#[path = "quota_storyboard_capture_test/native_adapter_fixture_support.rs"]
pub(crate) mod native_adapter_fixture_support;
