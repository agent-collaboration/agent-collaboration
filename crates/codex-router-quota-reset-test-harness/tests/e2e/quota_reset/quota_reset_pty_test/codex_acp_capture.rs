//! Installed Codex ACP journey over the shared public wrapper and grid line protocol. The TypeScript
//! run qualifies the declared target and writes its hash-bound launch configuration; this entry only
//! checks that binding, launches the existing public wrapper (inherited normal HOME and CODEX_HOME)
//! and relays the one PTY. It never declares a journey passed: grid checkpoints stay in TypeScript.
use super::capture_support::{
    digest_file, encode_json_string, permanent_adapter_source, verified_node_executable,
};
use super::grid_terminal_capture::{drive_terminal, emit};
use super::isolated_fixture_test::{TestResult, ensure};
use super::terminal_interaction_test::{KeyboardProtocol, TerminalDriver};
use sha2::{Digest, Sha256};
use std::fs;

fn required_codex_value(name: &str) -> TestResult<String> {
    let value = std::env::var(name)?;
    ensure(!value.is_empty(), "missing Codex ACP target value")?;
    Ok(value)
}

pub(super) fn run_codex_acp_grid_observation() -> TestResult<()> {
    let (node, node_sha256) = verified_node_executable()?;
    let output = fs::canonicalize(required_codex_value("TUI_GRID_CODEX_OUTPUT")?)?;
    let config = fs::canonicalize(required_codex_value("TUI_GRID_CODEX_CONFIG")?)?;
    ensure(
        config == output.join("codex-target.json"),
        "Codex configuration must belong to this output",
    )?;
    let config_sha256 = digest_file(&config)?;
    ensure(
        config_sha256 == required_codex_value("TUI_GRID_CODEX_CONFIG_SHA256")?,
        "Codex configuration binding changed",
    )?;
    let wrapper = permanent_adapter_source("fixture-child.ts")?;
    let wrapper_config = output.join("fixture-command.json");
    fs::copy(&config, &wrapper_config)?;
    let protocol = match required_codex_value("TUI_GRID_KEYS")?.as_str() {
        "legacy" => KeyboardProtocol::Legacy,
        "kitty" => KeyboardProtocol::Kitty,
        _ => return Err("unknown Codex key mode".into()),
    };
    let terminal = TerminalDriver::spawn_public_wrapper(&node, &wrapper, &wrapper_config, &output)?;
    let result = drive_terminal(terminal, protocol)?;
    if !result.completed {
        emit(&format!("STOPPED {}", result.bytes))?;
        return Err("Codex journey stopped before its normal exit".into());
    }
    ensure(
        fs::read_to_string(output.join("quota-child.exit"))?.trim() == "0",
        "Codex child failed",
    )?;
    ensure(
        !output.join("cleanup-unsettled").exists(),
        "Codex child cleanup unsettled",
    )?;
    for name in ["quota-child.pid", "quota-wrapper.pid"] {
        let pid: u32 = fs::read_to_string(output.join(name))?.trim().parse()?;
        ensure(
            !std::process::Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .output()?
                .status
                .success(),
            "owned Codex child survived cleanup",
        )?;
    }
    fs::write(
        output.join("codex-acp-receipt.json"),
        format!(
            "{{\"kind\":\"codex-acp-pty-lifecycle-only\",\"node_sha256\":\"{node_sha256}\",\"codex_capture_source_sha256\":{},\"fixed_child_sha256\":{},\"config_sha256\":{},\"child_exit\":0,\"owned_processes_stopped\":true,\"last_viewport\":{}}}\n",
            encode_json_string(&format!(
                "{:x}",
                Sha256::digest(include_bytes!("codex_acp_capture.rs"))
            )),
            encode_json_string(&digest_file(&wrapper)?),
            encode_json_string(&config_sha256),
            encode_json_string(fs::read_to_string(output.join("quota-child.viewport"))?.trim()),
        ),
    )?;
    emit(&format!("FINISHED {}", result.bytes))?;
    Ok(())
}
