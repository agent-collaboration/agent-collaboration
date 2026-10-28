//! Actual executable requests with explicit private roots and concurrent child capture.
use board_central_test_support::EnrollmentTestCentral;
use std::{
    error::Error, os::unix::fs::PermissionsExt, path::Path, process::Output, time::Duration,
};
type TestResult<T> = Result<T, Box<dyn Error>>;
pub const OWNER_ENV: &str = "CODEX_ROUTER_FLEET_OWNER_TOKEN";

pub fn command() -> tokio::process::Command {
    let runner =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo_debug_signing_runner.sh");
    let mut command = tokio::process::Command::new(runner);
    command.arg(env!("CARGO_BIN_EXE_codex-router")).arg("fleet");
    command.env_remove(OWNER_ENV);
    command.env("OTEL_SDK_DISABLED", "true");
    command
}

pub async fn run(command: tokio::process::Command) -> TestResult<Output> {
    codex_router_test_support::owned_cli_process::run_cli_output(command, Duration::from_secs(60))
        .await
        .map_err(std::io::Error::other)
        .map_err(Into::into)
}

pub fn enrollment(
    root: &Path,
    central: &EnrollmentTestCentral,
    expiry: &str,
) -> TestResult<tokio::process::Command> {
    let secret = root
        .parent()
        .ok_or("fixture parent missing")?
        .join("access-input");
    std::fs::write(&secret, central.access_client_secret())?;
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600))?;
    let mut command = command();
    command
        .args([
            "enroll",
            "--endpoint",
            central.endpoint(),
            "--label",
            "producer-fixture",
            "--access-client-id",
            central.access_client_id(),
            "--access-client-secret-file",
        ])
        .arg(secret)
        .args(["--expires-at", expiry, "--router-root"])
        .arg(root)
        .env(OWNER_ENV, central.owner_token());
    Ok(command)
}

pub async fn status(root: &Path) -> TestResult<Output> {
    let mut command = command();
    command.args(["status", "--router-root"]).arg(root);
    run(command).await
}

pub fn assert_private(output: &Output, sentinels: &[&str]) {
    for sentinel in sentinels {
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains(sentinel),
            "stdout exposed a credential"
        );
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains(sentinel),
            "stderr exposed a credential"
        );
    }
}

pub fn require_success(output: &Output) -> TestResult<serde_json::Value> {
    if !output.status.success() {
        return Err(format!(
            "CLI exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// A byte snapshot detects rewrites as well as additional secret generations.
pub fn snapshot(root: &Path) -> TestResult<Vec<(std::path::PathBuf, Vec<u8>)>> {
    fn visit(
        root: &Path,
        path: &Path,
        files: &mut Vec<(std::path::PathBuf, Vec<u8>)>,
    ) -> std::io::Result<()> {
        if !path.exists() {
            return Ok(());
        }
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                visit(root, &entry.path(), files)?;
            } else {
                files.push((
                    entry
                        .path()
                        .strip_prefix(root)
                        .map_err(std::io::Error::other)?
                        .to_owned(),
                    std::fs::read(entry.path())?,
                ));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    for directory in ["fleet", "secrets"] {
        visit(root, &root.join(directory), &mut files)?;
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}
