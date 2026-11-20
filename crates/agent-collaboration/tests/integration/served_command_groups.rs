//! Compiled CLI journal and native command groups over the served API.

use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_service::ApplicationFixture;
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc};

const SERVICE_ID: &str = "00000000-0000-4000-8000-000000000301";
const SERVICE_EPOCH: &str = "00000000-0000-4000-8000-000000000302";
const CALLER_SESSION_ID: &str = "served-command-caller";

struct ServedFixture {
    directory: tempfile::TempDir,
    served: ServedCollaborationApi,
    journal: Arc<lifecycle_observation::LifecycleStore>,
}

impl ServedFixture {
    #[expect(
        clippy::expect_used,
        reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
    )]
    /// An application with a lifecycle journal and a declared Codex endpoint.
    async fn start() -> Self {
        let directory = tempfile::tempdir_in("/tmp").expect("service directory");
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private service directory");
        let journal = lifecycle_observation::ObservationJournal::open(
            &directory.path().join("session-registry.sqlite"),
            "00000000-0000-4000-8000-000000000303"
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
        let served = ServedCollaborationApi::start(directory.path(), (identity).build())
            .expect("served collaboration API");
        Self {
            directory,
            served,
            journal,
        }
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }

    #[expect(
        clippy::expect_used,
        reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
    )]
    async fn finish(self) {
        self.served.stop().await.expect("collaboration API stops");
        if let Ok(journal) = Arc::try_unwrap(self.journal) {
            journal.close().await;
        }
    }
}

fn codex_endpoint() -> Value {
    json!({"serviceId":SERVICE_ID,"endpointId":"codex-local"})
}

#[expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
#[expect(
    clippy::panic,
    reason = "Broken fixture invariants must fail the test immediately."
)]
/// Runs the CLI as the caller Session and returns its exit status and one JSON record,
/// checked against the published finite-command contract.
async fn run_finite(directory: &Path, arguments: &[&str]) -> (Option<i32>, Value) {
    let (status, record) = run_json(directory, arguments).await;
    let schemas =
        collaboration_client::protocol::protocol_type_schemas().expect("protocol schema export");
    let validator = jsonschema::validator_for(
        schemas
            .get("FiniteCommandRecord")
            .expect("FiniteCommandRecord schema"),
    )
    .expect("finite record validator");
    if let Err(error) = validator.validate(&record) {
        panic!("{arguments:?} broke the published CLI record: {error}; {record}");
    }
    (status, record)
}

#[expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
#[expect(
    clippy::panic,
    reason = "Broken fixture invariants must fail the test immediately."
)]
/// Runs the CLI as the caller Session and returns its exit status and one JSON record.
async fn run_json(directory: &Path, arguments: &[&str]) -> (Option<i32>, Value) {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(arguments)
        .args(["--json", "--service-directory"])
        .arg(directory)
        .env("CODEX_THREAD_ID", CALLER_SESSION_ID)
        .env_remove("CODEX_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CURSOR_CONVERSATION_ID")
        .output()
        .await
        .expect("CLI runs");
    let record: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{arguments:?} printed no JSON record ({error}): stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code(), record)
}

#[tokio::test]
async fn journal_reads_through_the_served_api() {
    let fixture = ServedFixture::start().await;

    let (status, journal) = run_finite(fixture.path(), &["journal", "status"]).await;
    assert_eq!(status, Some(0), "{journal}");
    assert_eq!(journal["kind"], "result", "{journal}");

    fixture.finish().await;
}

#[tokio::test]
async fn native_session_commands_refuse_an_endpoint_without_a_native_route() {
    let fixture = ServedFixture::start().await;

    let (status, renamed) = run_finite(
        fixture.path(),
        &[
            "session",
            "rename",
            "--endpoint",
            "codex-local",
            "--session",
            "served-command-thread",
            "--name",
            "Renamed",
        ],
    )
    .await;
    assert_eq!(renamed["kind"], "error", "{renamed}");
    assert_eq!(
        renamed["error"]["kind"], "unsupportedCapability",
        "{renamed}"
    );
    assert_eq!(status, Some(2), "{renamed}");

    let (status, interrupted) = run_finite(
        fixture.path(),
        &[
            "turn",
            "interrupt",
            "--endpoint",
            "codex-local",
            "--session",
            "served-command-thread",
            "--turn",
            "served-command-turn",
        ],
    )
    .await;
    assert_eq!(interrupted["kind"], "error", "{interrupted}");
    assert_eq!(
        interrupted["error"]["kind"], "unsupportedCapability",
        "{interrupted}"
    );
    assert_eq!(status, Some(2), "{interrupted}");

    fixture.finish().await;
}
