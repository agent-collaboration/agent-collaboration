//! Controlled public transport proof, without a Router, Host or provider runtime.
use super::isolated_fixture_test::{TestResult, ensure};
use super::terminal_interaction_test::TerminalDriver;
use std::fs;
use std::path::Path;
use std::time::Duration;
#[path = "native_adapter_fixture_support.rs"]
#[expect(
    dead_code,
    reason = "Shared fixture also exposes the original native adapter control API"
)]
mod fixture_support;
use fixture_support::{NativeFixture, require_process_absent};

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

#[test]
fn changed_public_wrapper_identity_refuses_signal_and_child_can_finish_normally() -> TestResult<()>
{
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
    let identity_path = output.join("quota-wrapper.identity");
    let identity = fs::read(&identity_path)?;
    fs::write(&identity_path, b"changed UID birth command\n")?;
    let refused = terminal.signal_public_wrapper().is_err();
    fs::write(&identity_path, identity)?;
    fs::write(fixture.root.join("release-child"), "release\n")?;
    terminal.finish(Duration::from_secs(8))?;
    ensure(refused, "changed public wrapper identity accepted signal")?;
    ensure(
        fixture.read("quota-child.exit")? == "0",
        "refused signal still terminated the child",
    )?;
    fixture.cleanup()
}
