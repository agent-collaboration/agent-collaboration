//! Exercise the installed command vocabulary through the actual CLI executable.
// Assertions prove the public result; fixture I/O remains fallible.
#![deny(clippy::too_many_lines)]
use std::{
    error::Error,
    path::Path,
    process::{Command, Output},
};

type TestResult = Result<(), Box<dyn Error>>;

fn command(root: &Path, arguments: &[&str]) -> Result<Output, Box<dyn Error>> {
    let runner =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo_debug_signing_runner.sh");
    Ok(Command::new(runner)
        .arg(env!("CARGO_BIN_EXE_codex-router"))
        .args(["fleet"])
        .args(arguments)
        .arg("--router-root")
        .arg(root)
        .env(
            "CODEX_ROUTER_FLEET_OWNER_TOKEN",
            "owner-token-must-stay-private",
        )
        .output()?)
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn fleet_status_of_an_absent_file_is_public_metadata_only() -> TestResult {
    let root = tempfile::tempdir()?;
    let output = command(root.path(), &["status"])?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(status["status"]["kind"], "absent");
    assert_eq!(status["activation"], "nextRouterStart");
    assert!(!root.path().join("agent-communication").exists());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("owner-token-must-stay-private"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("owner-token-must-stay-private"));
    Ok(())
}
