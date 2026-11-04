//! Public entry retains its fixed child/Host/effect contract; only the sink changes.
use super::capture_support::{
    digest_file, encode_json_string, permanent_adapter_source, verified_node_executable,
};
use super::grid_terminal_capture::{drive_terminal, emit};
use super::isolated_fixture_test::{TestResult, ensure};
use super::terminal_interaction_test::{KeyboardProtocol, TerminalDriver};
use portable_pty::CommandBuilder;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
fn required_public_value(name: &str) -> TestResult<String> {
    let value = std::env::var(name)?;
    ensure(!value.is_empty(), "missing public target value")?;
    Ok(value)
}
pub(super) fn run_live_public_sessions() -> TestResult<()> {
    let (node, node_sha256) = verified_node_executable()?;
    ensure(
        std::env::var_os("QUOTA_STORYBOARD_COMPONENT_FIXTURE_BINARY").is_none(),
        "public and synthetic component variants cannot be mixed",
    )?;
    let config = fs::canonicalize(required_public_value(
        "QUOTA_STORYBOARD_PUBLIC_SESSIONS_CONFIG",
    )?)?;
    let root = fs::canonicalize(required_public_value("QUOTA_STORYBOARD_OUTPUT")?)?;
    ensure(
        config == root.join("public-target.json"),
        "public configuration must belong to this output",
    )?;
    ensure(
        digest_file(&config)?
            == required_public_value("QUOTA_STORYBOARD_PUBLIC_SESSIONS_CONFIG_SHA256")?,
        "public configuration binding changed",
    )?;
    let binary = fs::canonicalize(required_public_value(
        "QUOTA_STORYBOARD_PUBLIC_SESSIONS_BINARY",
    )?)?;
    ensure(binary.is_file(), "public binary missing")?;
    let service = required_public_value("QUOTA_STORYBOARD_PUBLIC_SESSIONS_SERVICE_ID")?;
    let epoch = required_public_value("QUOTA_STORYBOARD_PUBLIC_SESSIONS_SERVICE_EPOCH")?;
    let effects = required_public_value("QUOTA_STORYBOARD_PUBLIC_SESSIONS_EFFECT_SHA256")?;
    ensure(
        effects.len() == 64 && effects.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "public effect proof binding required",
    )?;
    let output = root.join("cua-sessions");
    ensure(output.is_dir(), "fresh public output required")?;
    let wrapper = permanent_adapter_source("fixture-child.ts")?;
    let wrapper_config = output.join("fixture-command.json");
    fs::copy(&config, &wrapper_config)?;
    let protocol = match required_public_value("TUI_GRID_KEYS")?.as_str() {
        "legacy" => KeyboardProtocol::Legacy,
        "kitty" => KeyboardProtocol::Kitty,
        _ => return Err("unknown public key mode".into()),
    };
    let terminal = TerminalDriver::spawn_public_wrapper(&node, &wrapper, &wrapper_config, &output)?;
    let result = drive_terminal(terminal, protocol)?;
    if !result.completed {
        emit(&format!("STOPPED {}", result.bytes))?;
        return Err("public CUA stopped early; job state remains with runtime helper".into());
    }
    ensure(
        fs::read_to_string(output.join("quota-child.exit"))?.trim() == "0",
        "public child failed",
    )?;
    ensure(
        !output.join("cleanup-unsettled").exists(),
        "public child cleanup unsettled",
    )?;
    for name in ["quota-child.pid", "quota-wrapper.pid"] {
        let pid: u32 = fs::read_to_string(output.join(name))?.trim().parse()?;
        ensure(
            !std::process::Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .output()?
                .status
                .success(),
            "owned public child survived cleanup",
        )?;
    }
    verified_node_executable()?;
    let count: usize = required_public_value("TUI_GRID_PUBLIC_ARTIFACT_COUNT")?.parse()?;
    fs::write(
        output.join("public-cua-receipt.json"),
        format!(
            "{{\"kind\":\"public-sessions-cua-lifecycle-only\",\"binary_sha256\":{},\"node_sha256\":\"{node_sha256}\",\"public_capture_source_sha256\":{},\"fixed_child_sha256\":{},\"host_service_id\":{},\"host_service_epoch\":{},\"child_exit\":0,\"owned_processes_stopped\":true,\"artifact_sink_stopped\":true,\"artifact_count\":{count},\"last_viewport\":{},\"effect_evidence_sha256\":{},\"job_settlement\":\"separate-runtime-helper-proof-required\"}}\n",
            encode_json_string(&digest_file(&binary)?),
            encode_json_string(&format!(
                "{:x}",
                Sha256::digest(include_bytes!("public_sessions_capture.rs"))
            )),
            encode_json_string(&digest_file(&wrapper)?),
            encode_json_string(&service),
            encode_json_string(&epoch),
            encode_json_string(fs::read_to_string(output.join("quota-child.viewport"))?.trim()),
            encode_json_string(&effects)
        ),
    )?;
    emit(&format!("FINISHED {}", result.bytes))?;
    Ok(())
}

impl TerminalDriver {
    pub(super) fn spawn_public_wrapper(
        node: &Path,
        wrapper: &Path,
        configuration: &Path,
        output: &Path,
    ) -> TestResult<Self> {
        let mut command = CommandBuilder::new(node);
        command.arg(wrapper);
        command.arg(configuration);
        // Public entry keeps the real inherited environment, including absent CODEX_HOME.
        command.env("QUOTA_STORYBOARD_FIXTURE_COMMAND", configuration);
        command.env("TERM", "xterm-256color");
        command.env("LANG", "C.UTF-8");
        command.cwd(output);
        Self::spawn_prepared_command(command, Some(output.to_path_buf()))
    }
}
