//! One existing wrapper/PTY route for separately declared showcase and effectful quota entries.
use super::{KeyboardProtocol, TerminalDriver, TestResult, drive_terminal, emit, ensure};
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum WrappedFixtureSurface {
    Showcase,
    QuotaEffects,
}
#[derive(serde::Deserialize)]
struct WrappedFixtureConfiguration {
    surface: WrappedFixtureSurface,
    binary: std::path::PathBuf,
    binary_sha256: String,
}
pub(super) fn run(
    protocol: KeyboardProtocol,
    check_only: bool,
    surface: WrappedFixtureSurface,
) -> TestResult<()> {
    use super::super::capture_support::{
        digest_file, permanent_adapter_source, verified_node_executable,
    };
    let (prefix, label) = match surface {
        WrappedFixtureSurface::Showcase => ("TUI_SHOWCASE", "showcase"),
        WrappedFixtureSurface::QuotaEffects => ("TUI_QUOTA_EFFECT", "quota-effects"),
    };
    let (node, _) = verified_node_executable()?;
    let output = std::fs::canonicalize(
        std::env::var_os(format!("{prefix}_OUTPUT")).ok_or("wrapped fixture output required")?,
    )?;
    let configuration = std::fs::canonicalize(
        std::env::var_os(format!("{prefix}_CONFIGURATION"))
            .ok_or("wrapped fixture configuration required")?,
    )?;
    ensure(
        configuration == output.join("fixture-command.json"),
        "wrapped fixture configuration must belong to this output",
    )?;
    ensure(
        digest_file(&configuration)? == std::env::var(format!("{prefix}_CONFIGURATION_SHA256"))?,
        "wrapped fixture configuration changed",
    )?;
    let nomination: WrappedFixtureConfiguration =
        serde_json::from_slice(&std::fs::read(&configuration)?)?;
    ensure(
        nomination.surface == surface,
        "wrapped fixture surface does not match the declared driver route",
    )?;
    super::verify_product_binary(&nomination.binary, &nomination.binary_sha256)?;
    let product_path = std::env::var_os("TUI_GRID_PRODUCT_BINARY");
    let product_digest = std::env::var("TUI_GRID_PRODUCT_SHA256").ok();
    let product =
        super::nominated_product_binary(product_path.as_deref(), product_digest.as_deref())?;
    if surface == WrappedFixtureSurface::QuotaEffects {
        ensure(
            product.as_ref() == Some(&nomination.binary),
            "effectful quota requires its same declared product artifact",
        )?;
        ensure(
            product_digest.as_ref() == Some(&nomination.binary_sha256),
            "effectful quota product binding differs from wrapper configuration",
        )?;
    }
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
            "wrapped fixture launch admission failed: {}",
            String::from_utf8_lossy(&admission.stderr)
        )
        .into());
    }
    if check_only {
        return emit(&format!(
            "CHECKED {label} {} 120 36",
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
        "wrapped fixture child failed",
    )?;
    ensure(
        !output.join("cleanup-unsettled").exists(),
        "wrapped fixture cleanup unsettled",
    )?;
    for name in ["quota-child.pid", "quota-wrapper.pid"] {
        let pid: u32 = std::fs::read_to_string(output.join(name))?.trim().parse()?;
        ensure(
            !std::process::Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .output()?
                .status
                .success(),
            "owned wrapped fixture process survived cleanup",
        )?;
    }
    emit(&format!("FINISHED {}", result.bytes))
}
