mod quota_reset_pty_test {
    use std::{fs, path::Path};
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

        ensure(
            !cli_manifest.contains("codex-router-quota-reset-test-harness"),
            "production CLI manifest still owns the harness executable",
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
