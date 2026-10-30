//! Controlled public transport proof, without a Router, Host or provider runtime.
use super::isolated_fixture_test::{TestResult, ensure};
use super::terminal_interaction_test::TerminalDriver;
use crate::native_adapter_fixture_support as fixture_support;
use fixture_support::{NativeFixture, require_process_absent};
use std::fs;
use std::path::Path;
use std::time::Duration;

#[test]
fn public_grid_constructor_preserves_home_and_guarded_stop_reaps_its_child() -> TestResult<()> {
    let fixture = NativeFixture::prepare("public-held")?;
    let output = fixture.root.join("output");
    let configuration = output.join("fixture-command.json");
    fs::copy(fixture.root.join("configuration.json"), &configuration)?;
    let wrapper = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/verification/tui/fixture-child.ts")
        .canonicalize()?;
    let mut terminal =
        TerminalDriver::spawn_public_wrapper(&fixture.node, &wrapper, &configuration, &output)?;
    terminal.wait_for_text("NATIVE_ADAPTER_READY", Duration::from_secs(8))?;
    ensure(
        fixture.read("home.txt")? == std::env::var("HOME").unwrap_or_else(|_| "absent".to_owned()),
        "public grid changed normal HOME",
    )?;
    ensure(
        fixture.read("codex-home.txt")?
            == std::env::var("CODEX_HOME").unwrap_or_else(|_| "absent".to_owned()),
        "public grid changed CODEX_HOME presence/value",
    )?;
    let child = fixture.read("quota-child.pid")?;
    let wrapper_pid = fixture.read("quota-wrapper.pid")?;
    terminal.terminate_and_reap(Duration::from_secs(8))?;
    ensure(
        fixture.read("quota-child.exit")? == "-15",
        "guarded stop did not preserve wrapper child termination",
    )?;
    require_process_absent(&child)?;
    require_process_absent(&wrapper_pid)?;
    fixture.cleanup()
}
/// TerminalDriver gives the wrapper LANG=C.UTF-8, so a parent whose LANG alone (no LC_TIME or
/// LC_ALL) carries a day-before-month date order is the case that split the receipt writer from
/// its signal reader. The guarded stop must still recognise its own wrapper there.
#[test]
fn public_wrapper_receipt_survives_a_non_c_parent_locale() -> TestResult<()> {
    let result = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "public_grid_transport_test::public_grid_constructor_preserves_home_and_guarded_stop_reaps_its_child",
            "--nocapture",
        ])
        .env_remove("LC_ALL")
        .env_remove("LC_TIME")
        .env("LANG", "en_CA.UTF-8")
        .output()?;
    if !result.status.success() {
        return Err(format!(
            "public wrapper stop failed under parent LANG=en_CA.UTF-8: {}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(())
}
