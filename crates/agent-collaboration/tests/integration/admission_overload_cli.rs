//! A request the API sheds at its request limit was not run. The CLI reports it as the
//! retryable `overloaded` failure, never as a connection failure or protocol violation.

use collaboration_mcp::test_support::{ServedCollaborationApi, TestServeOptions};
use collaboration_service::ApplicationFixture;
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc, time::Duration};

const SERVICE_ID: &str = "00000000-0000-4000-8000-000000000001";
const SERVICE_EPOCH: &str = "00000000-0000-4000-8000-000000000002";

fn codex_endpoint() -> Value {
    json!({"serviceId": SERVICE_ID, "endpointId": "codex-local"})
}

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
    // Arrange: one request slot, held by a caught-up journal read waiting for new records.
    let directory = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .expect("private directory");
    let journal = lifecycle_observation::ObservationJournal::open(
        &directory.path().join("session-registry.sqlite"),
        "00000000-0000-4000-8000-000000000003"
            .to_owned()
            .try_into()
            .expect("journal identity"),
    )
    .await
    .expect("journal");
    let journal = Arc::new(lifecycle_observation::LifecycleStore::new(journal));
    let description = serde_json::from_value(json!({
        "endpoint":codex_endpoint(),"label":"Codex fixture",
        "availability":{"state":"available","observedAt":"2026-10-08T00:00:00Z"},
        "channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"codex-native.sock",
            "schemaDigest":null,"generation":{"serviceEpoch":SERVICE_EPOCH,"generation":1}}]
    }))
    .expect("Codex endpoint");
    let identity = ApplicationFixture::new(SERVICE_ID, SERVICE_EPOCH)
        .expect("service identity")
        .with_endpoints(vec![description])
        .expect("endpoint inventory")
        .with_journal(Arc::clone(&journal));
    let served = Arc::new(
        ServedCollaborationApi::start_with(
            directory.path(),
            identity.build(),
            TestServeOptions {
                concurrent_requests: Some(1),
                ..TestServeOptions::default()
            },
        )
        .expect("served API"),
    );
    let status = served
        .call("journal_status", json!({}))
        .await
        .expect("journal status");
    let journal_id = status["result"]["bounds"]["journalId"].clone();
    let last_sequence = status["result"]["bounds"]["lastSequence"].clone();
    assert!(journal_id.is_string(), "{status}");
    let held = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let holder = Arc::clone(&served);
            let read = json!({
                "endpoint": codex_endpoint(),
                "after": {"journalId": journal_id, "sequence": last_sequence},
                "pageSize": 1,
                "waitMilliseconds": 30000
            });
            let held = tokio::spawn(async move {
                let _answer = holder.call("journal_read", read).await;
            });
            tokio::time::sleep(Duration::from_millis(100)).await;
            if held.is_finished() {
                continue;
            }
            let probe = served
                .call("journal_status", json!({}))
                .await
                .expect("probe");
            if probe["error"]["data"]["kind"] == "overloaded" {
                break Some(held);
            }
            held.abort();
        }
    })
    .await
    .ok()
    .flatten()
    .expect("the journal read holds the only request slot");

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

    // Assert: the untyped command reports the retryable overload with no effect.
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
    held.abort();
    let _ended = held.await;
    if let Ok(served) = Arc::try_unwrap(served) {
        served.stop().await.expect("API stops");
    }
    if let Ok(journal) = Arc::try_unwrap(journal) {
        journal.close().await;
    }
}
