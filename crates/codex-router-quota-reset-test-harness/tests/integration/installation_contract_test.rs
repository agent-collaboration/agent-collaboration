mod quota_reset_pty_test {
    use std::{fs, path::Path, process::Command};
    pub(super) type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

    pub(super) fn ensure(condition: bool, message: &'static str) -> TestResult<()> {
        if condition {
            Ok(())
        } else {
            Err(std::io::Error::other(message).into())
        }
    }

    #[test]
    fn production_cli_manifest_has_no_harness_install_target_or_pty_dependency() -> TestResult<()> {
        let harness_manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let cli_manifest =
            fs::read_to_string(harness_manifest_root.join("../codex-router-cli/Cargo.toml"))?;
        let harness_manifest = fs::read_to_string(harness_manifest_root.join("Cargo.toml"))?;
        let workspace_manifest =
            fs::read_to_string(harness_manifest_root.join("../../Cargo.toml"))?;

        // D3 permits the harness library only as a dev-dependency; installed
        // CLI targets and normal/build dependencies must still exclude it.
        let metadata = Command::new("cargo")
            .args([
                "metadata",
                "--locked",
                "--offline",
                "--no-deps",
                "--format-version",
                "1",
            ])
            .current_dir(harness_manifest_root.join("../.."))
            .output()?;
        ensure(metadata.status.success(), "Cargo metadata must succeed")?;
        let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout)?;
        let cli = metadata
            .get("packages")
            .and_then(serde_json::Value::as_array)
            .and_then(|packages| {
                packages
                    .iter()
                    .find(|package| package["name"] == "codex-router-cli")
            })
            .ok_or("CLI package missing from workspace metadata")?;
        let targets = cli
            .get("targets")
            .and_then(serde_json::Value::as_array)
            .ok_or("CLI targets missing")?;
        ensure(
            targets.iter().all(|target| {
                target["name"] != "codex-router-quota-reset-test-harness"
                    && target["src_path"]
                        .as_str()
                        .is_some_and(|path| !path.contains("quota-reset-test-harness"))
            }),
            "production CLI manifest still owns the harness executable",
        )?;
        let dependencies = cli
            .get("dependencies")
            .and_then(serde_json::Value::as_array)
            .ok_or("CLI dependencies missing")?;
        ensure(
            dependencies.iter().all(|dependency| {
                dependency["name"] != "codex-router-quota-reset-test-harness"
                    || dependency["kind"] == "dev"
            }),
            "production CLI normal/build dependency includes the harness",
        )?;
        ensure(
            !cli_manifest.contains("portable-pty"),
            "production CLI manifest still owns the PTY dependency",
        )?;
        ensure(
            harness_manifest.contains("publish = false"),
            "dedicated harness package is publishable",
        )?;
        ensure(
            workspace_manifest.contains("crates/codex-router-quota-reset-test-harness"),
            "workspace does not own the dedicated harness package",
        )?;
        Ok(())
    }
}
