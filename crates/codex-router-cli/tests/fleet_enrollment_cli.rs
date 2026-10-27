#![expect(
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Exercise the installed command vocabulary through the actual CLI executable.
#![allow(clippy::panic_in_result_fn)] // Assertions prove the public result; fixture I/O remains fallible.
#![deny(clippy::too_many_lines)]
use std::{
    error::Error,
    os::unix::fs::PermissionsExt,
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

#[test]
fn fleet_enroll_requires_explicit_expiry_and_preserves_existing_file() -> TestResult {
    let root = tempfile::tempdir()?;
    let fleet = root.path().join("fleet");
    std::fs::create_dir(&fleet)?;
    let retained = fleet.join("enrollment.json");
    std::fs::write(&retained, b"retained-enrollment-fixture")?;
    std::fs::set_permissions(&retained, std::fs::Permissions::from_mode(0o600))?;
    let secret = root.path().join("access-secret");
    std::fs::write(&secret, "access-secret-must-stay-private")?;
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600))?;
    let output = command(
        root.path(),
        &[
            "enroll",
            "--endpoint",
            "ws://127.0.0.1:1/v1/session",
            "--label",
            "fixture",
            "--access-client-id",
            "fixture.access",
            "--access-client-secret-file",
            secret.to_str().ok_or("secret path")?,
        ],
    )?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--expires-at"));
    assert_eq!(std::fs::read(retained)?, b"retained-enrollment-fixture");
    for secret in [
        "access-secret-must-stay-private",
        "owner-token-must-stay-private",
    ] {
        assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
    }
    Ok(())
}
