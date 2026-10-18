//! Execute the actual CLI against the test-owned service manifest and socket.
use serde_json::{Value, json};
use std::{path::Path, process::Command};
type TestError = Box<dyn std::error::Error + Send + Sync>;
pub async fn cli(directory: &Path, args: &[&str]) -> Result<Value, TestError> {
    cli_with_session(directory, args, None).await
}
pub async fn cli_with_session(
    directory: &Path,
    args: &[&str],
    session: Option<&str>,
) -> Result<Value, TestError> {
    let arguments = args
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect::<Vec<_>>();
    let directory = directory.to_owned();
    let session = session.map(str::to_owned);
    let output = tokio::task::spawn_blocking(move || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"));
        if let Some(session) = session {
            command
                .env("CODEX_THREAD_ID", session)
                .env_remove("CLAUDE_CODE_SESSION_ID")
                .env_remove("CURSOR_CONVERSATION_ID");
        }
        command
            .args(arguments)
            .arg("--service-directory")
            .arg(directory)
            .arg("--json")
            .output()
    })
    .await??;
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}
pub fn accepted(value: &Value) {
    assert_eq!(
        value.pointer("/result/record/result/decision/kind"),
        Some(&json!("accepted")),
        "{value}"
    )
}
