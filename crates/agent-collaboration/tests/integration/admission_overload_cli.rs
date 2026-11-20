//! A request the API sheds at its request limit was not run. The CLI reports it as an
//! `overloaded` failure to retry later: a board command in the board's typed refusal, and an
//! untyped command as the retryable overload, never as a connection failure or protocol
//! violation.

use collaboration_mcp::request_slot_holder::RequestSlotHolder;
use collaboration_mcp::test_support::{ServedCollaborationApi, TestServeOptions};
use collaboration_service::ApplicationFixture;

use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc, time::Duration};

const SERVICE_ID: &str = "00000000-0000-4000-8000-000000000001";

async fn run_cli(
    directory: &Path,
    arguments: &[&str],
) -> Result<(Option<i32>, Value), Box<dyn std::error::Error>> {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(arguments)
        .arg("--service-directory")
        .arg(directory)
        .arg("--json")
        .output()
        .await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().last().unwrap_or_default();
    Ok((output.status.code(), serde_json::from_str(line)?))
}

#[tokio::test]
async fn shed_commands_report_an_overload_to_retry() {
    // Arrange: one request slot, held by the slot holder.
    let directory = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .expect("private directory");
    let identity = ApplicationFixture::new(SERVICE_ID, "00000000-0000-4000-8000-000000000002")
        .expect("service identity");
    let (identity, holder) = RequestSlotHolder::install(identity, SERVICE_ID, directory.path())
        .await
        .expect("slot holder");
    let served = Arc::new(
        ServedCollaborationApi::start_with(
            directory.path(),
            (identity).build(),
            TestServeOptions {
                concurrent_requests: Some(1),
                ..TestServeOptions::default()
            },
        )
        .expect("served API"),
    );
    // The holding call keeps the only request slot once a probe sent after it is shed. A
    // probe that wins the slot first sheds the holding call instead, which then ends, so it
    // is started again.
    let hold = |served: &Arc<ServedCollaborationApi>| {
        let served = Arc::clone(served);
        let arguments = holder.arguments();
        tokio::spawn(async move {
            let _answer = served.call(RequestSlotHolder::TOOL, arguments).await;
        })
    };
    let held = tokio::time::timeout(Duration::from_secs(10), async {
        let mut held = hold(&served);
        loop {
            let probe = served
                .call("endpoints_list", json!({}))
                .await
                .expect("probe");
            if probe["error"]["data"]["kind"] == "overloaded" {
                break Some(held);
            }
            if held.is_finished() {
                held = hold(&served);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .ok()
    .flatten()
    .expect("the holding call holds the only request slot");

    // Act
    let (endpoints_exit, endpoints) = run_cli(directory.path(), &["endpoints", "list"])
        .await
        .expect("endpoints command output");
    let native = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["native", "--endpoint", "codex-local", "--service-directory"])
        .arg(directory.path())
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .expect("native bridge output");

    assert_eq!(endpoints_exit, Some(3), "{endpoints}");
    assert_eq!(endpoints["error"]["kind"], "overloaded", "{endpoints}");
    assert_eq!(endpoints["error"]["effect"], "none", "{endpoints}");
    assert_eq!(
        endpoints["error"]["nextAction"], "retryLater",
        "{endpoints}"
    );
    let native_stderr = String::from_utf8_lossy(&native.stderr);
    assert_eq!(native.status.code(), Some(3), "{native_stderr}");
    assert!(
        native_stderr.contains("Error: overloaded")
            && native_stderr.contains("Next action: retryLater"),
        "the native bridge names the retryable overload: {native_stderr}"
    );
    holder.release().await.expect("release the slot");
    let _ended = held.await;
    if let Ok(served) = Arc::try_unwrap(served) {
        served.stop().await.expect("API stops");
    }
}
