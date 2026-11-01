//! Admission and PTY launch for the separately declared ordinary component example.
use super::{KeyboardProtocol, TerminalDriver, TestResult, drive_terminal, emit, ensure};
pub(super) fn run(protocol: KeyboardProtocol, check_only: bool) -> TestResult<()> {
    use super::super::capture_support::{
        digest_file, permanent_adapter_source, verified_node_executable,
    };
    let (node, _) = verified_node_executable()?;
    let output = std::fs::canonicalize(
        std::env::var_os("TUI_SHOWCASE_OUTPUT").ok_or("showcase output required")?,
    )?;
    let configuration = std::fs::canonicalize(
        std::env::var_os("TUI_SHOWCASE_CONFIGURATION").ok_or("showcase configuration required")?,
    )?;
    ensure(
        configuration == output.join("fixture-command.json"),
        "showcase configuration must belong to this output",
    )?;
    ensure(
        digest_file(&configuration)? == std::env::var("TUI_SHOWCASE_CONFIGURATION_SHA256")?,
        "showcase configuration changed",
    )?;
    let wrapper = permanent_adapter_source("fixture-child.ts")?;
    let admission = std::process::Command::new(&node)
        .args([
            wrapper.as_os_str(),
            configuration.as_os_str(),
            std::ffi::OsStr::new("--check-only"),
        ])
        .env("QUOTA_STORYBOARD_FIXTURE_COMMAND", &configuration)
        .output()?;
    if !admission.status.success() {
        return Err(format!(
            "showcase launch admission failed: {}",
            String::from_utf8_lossy(&admission.stderr)
        )
        .into());
    }
    if check_only {
        return emit(&format!(
            "CHECKED showcase {} 120 36",
            std::env::var("TUI_GRID_KEYS")?
        ));
    }
    let terminal = TerminalDriver::spawn_public_wrapper(&node, &wrapper, &configuration, &output)?;
    let result = drive_terminal(terminal, protocol)?;
    if !result.completed {
        emit(&format!("STOPPED {}", result.bytes))?;
        return Ok(());
    }
    ensure(
        std::fs::read_to_string(output.join("quota-child.exit"))?.trim() == "0",
        "showcase child failed",
    )?;
    ensure(
        !output.join("cleanup-unsettled").exists(),
        "showcase cleanup unsettled",
    )?;
    for name in ["quota-child.pid", "quota-wrapper.pid"] {
        let pid: u32 = std::fs::read_to_string(output.join(name))?.trim().parse()?;
        ensure(
            !std::process::Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .output()?
                .status
                .success(),
            "owned showcase process survived cleanup",
        )?;
    }
    emit(&format!("FINISHED {}", result.bytes))
}
