use collaboration_client::board::*;

use serde_json::Value;
use serde_json::json;
use std::path::Path;
use std::time::Duration;

#[path = "../thread_subscription_cli/wait_uncertainty.rs"]
mod wait_uncertainty;

const SERVICE_ID: &str = "00000000-0000-4000-8000-000000000101";
const SERVICE_EPOCH: &str = "00000000-0000-4000-8000-000000000102";
const SESSION_ID: &str = "thread-subscription-cli";
type TestError = Box<dyn std::error::Error + Send + Sync>;
type TestResult = Result<(), TestError>;

async fn run_cli(
    service_directory: &Path,
    arguments: &[&str],
) -> Result<std::process::Output, TestError> {
    Ok(tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(arguments)
            .arg("--service-directory")
            .arg(service_directory)
            .arg("--json")
            .env("CODEX_THREAD_ID", SESSION_ID)
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CURSOR_CONVERSATION_ID")
            .kill_on_drop(true)
            .output(),
    )
    .await??)
}

fn ensure(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
