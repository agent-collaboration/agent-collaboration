use crate::update_support::UpdateResult as TestResult;
use owned_process_test_support::owned_cli_process::run_cli_output;
use std::{path::Path, time::Duration};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn version_is_the_released_executable_name_and_workspace_version() -> TestResult {
    // Arrange
    let runner =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo_debug_signing_runner.sh");
    // Act
    let mut command = tokio::process::Command::new(runner);
    command
        .arg(env!("CARGO_BIN_EXE_agent-router-update"))
        .arg("--version");
    let output = run_cli_output(command, Duration::from_secs(30))
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    // Assert
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout)?,
        format!("agent-router-update {}\n", env!("CARGO_PKG_VERSION"))
    );
    Ok(())
}
