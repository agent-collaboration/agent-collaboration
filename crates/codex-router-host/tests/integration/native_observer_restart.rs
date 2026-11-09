//! A native observer that connects but never restores coverage is a failed attempt: it
//! retries on the 1 s doubling backoff with one warning for the run of failures, and only
//! an observer that restored coverage resets the backoff.
use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use futures_util::{SinkExt as _, StreamExt as _};
use std::{
    os::unix::fs::PermissionsExt as _,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::UnixListener, sync::mpsc};
use tokio_tungstenite::tungstenite::Message;
use tracing_subscriber::layer::SubscriberExt as _;

/// The warning an observer that never restored coverage logs once per run of failures.
const NEVER_RECOVERED: &str = "native lifecycle observation failed before restoring coverage";

/// Inventory requests that fail before the observer can restore coverage.
const FAILED_INVENTORIES: usize = 4;

/// What the observer reported: every warning's message, and each backoff it chose.
#[derive(Default)]
struct Reports {
    warnings: Vec<String>,
    delays: Vec<Duration>,
}

/// Records warnings and the restart loop's chosen backoffs.
struct ObserverReports(Arc<Mutex<Reports>>);
impl<TSubscriber: tracing::Subscriber> tracing_subscriber::Layer<TSubscriber> for ObserverReports {
    #[expect(clippy::panic, reason = "A poisoned report lock fails the test.")]
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _: tracing_subscriber::layer::Context<'_, TSubscriber>,
    ) {
        #[derive(Default)]
        struct Fields {
            message: String,
            delay_ms: Option<u64>,
        }
        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.message = format!("{value:?}");
                }
            }
            fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
                if field.name() == "delay_ms" {
                    self.delay_ms = Some(value);
                }
            }
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut reports = self
            .0
            .lock()
            .unwrap_or_else(|error| panic!("reports: {error}"));
        if *event.metadata().level() == tracing::Level::WARN {
            reports.warnings.push(fields.message);
        } else if event.metadata().target() == "native_observer"
            && let Some(delay_ms) = fields.delay_ms
        {
            reports.delays.push(Duration::from_millis(delay_ms));
        }
    }
}

/// What the scripted app-server does with one connection's inventory request.
#[derive(Clone, Copy)]
enum Inventory {
    Fail,
    /// Restores coverage, then ends the connection.
    RecoverThenClose,
    /// Restores coverage and keeps the connection open.
    RecoverAndStay,
}

/// Serves one connection per scripted step, announcing each connection.
#[expect(
    clippy::panic,
    reason = "A scripted app-server that cannot serve fails the test."
)]
async fn serve(
    listener: UnixListener,
    script: Vec<Inventory>,
    accepted: mpsc::UnboundedSender<()>,
) {
    for step in script {
        let (stream, _) = listener
            .accept()
            .await
            .unwrap_or_else(|error| panic!("accept: {error}"));
        let _ = accepted.send(());
        let mut socket = tokio_tungstenite::accept_async(stream)
            .await
            .unwrap_or_else(|error| panic!("upgrade: {error}"));
        for method in ["initialize", "initialized", "thread/loaded/list"] {
            // The Host's shutdown may end the last connection at any point.
            let Some(Ok(frame)) = socket.next().await else {
                return;
            };
            let request: serde_json::Value = serde_json::from_str(
                frame
                    .to_text()
                    .unwrap_or_else(|error| panic!("text: {error}")),
            )
            .unwrap_or_else(|error| panic!("JSON: {error}"));
            assert_eq!(
                request.get("method").and_then(serde_json::Value::as_str),
                Some(method)
            );
            let id = request.get("id").cloned().unwrap_or_default();
            let reply = match (method, step) {
                ("initialized", _) => continue,
                ("initialize", _) => serde_json::json!({"id":id,"result":{}}),
                (_, Inventory::Fail) => serde_json::json!({
                    "id":id,"error":{"code":-32603,"message":"inventory unavailable"}
                }),
                _ => serde_json::json!({
                    "id":id,"result":{"data":[],"nextCursor":null}
                }),
            };
            if socket
                .send(Message::Text(reply.to_string().into()))
                .await
                .is_err()
            {
                return;
            }
        }
        match step {
            Inventory::Fail | Inventory::RecoverThenClose => drop(socket),
            Inventory::RecoverAndStay => {
                let _retired = socket.next().await;
                return;
            }
        }
    }
}

/// A schema export the observer's operations validate against.
#[expect(
    clippy::panic,
    reason = "A schema export the fixture cannot build fails the test."
)]
async fn schema_export(
    root: &std::path::Path,
) -> (
    codex_native_integration::ExecutableIdentity,
    codex_native_integration::NativeSchemaExport,
) {
    let executable = root.join("schema-generator");
    let mut definitions = serde_json::Map::new();
    for name in [
        "ThreadRead",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
    ] {
        for suffix in ["Params", "Response"] {
            definitions.insert(
                format!("{name}{suffix}"),
                serde_json::json!({"type":"object"}),
            );
        }
    }
    let schema = serde_json::json!({"definitions":{"v2":definitions}});
    std::fs::write(
        &executable,
        format!("#!/bin/sh\ncat > \"$5/codex_app_server_protocol.schemas.json\" <<'SCHEMA'\n{schema}\nSCHEMA\n"),
    )
    .unwrap_or_else(|error| panic!("generator: {error}"));
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("generator permissions: {error}"));
    let identity = codex_native_integration::executable_identity(&executable)
        .await
        .unwrap_or_else(|error| panic!("identity: {error}"));
    let export = codex_native_integration::NativeSchemaExport::generate(
        &identity,
        &root.join("schema-export"),
    )
    .await
    .unwrap_or_else(|error| panic!("export: {error}"));
    (identity, export)
}

#[tokio::test]
async fn an_observer_that_never_restores_coverage_backs_off_with_one_warning_until_it_recovers() {
    let reports = Arc::new(Mutex::new(Reports::default()));
    let _subscriber = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(ObserverReports(Arc::clone(&reports))),
    );
    let directory = tempfile::Builder::new()
        .prefix("observer-restart-")
        .tempdir_in("/tmp")
        .unwrap_or_else(|error| panic!("directory: {error}"));
    let root = directory.path().to_path_buf();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("directory permissions: {error}"));
    let mut runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: None,
        directory: root.clone(),
        codex_home: root.clone(),
        backend_socket: root.join("backend.sock"),
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        native_schema: None,
        peer_registry_directory: None,
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await
    .unwrap_or_else(|error| panic!("runtime start: {error}"));
    let (identity, export) = schema_export(&root).await;
    let listener = UnixListener::bind(root.join("backend.sock"))
        .unwrap_or_else(|error| panic!("backend fixture: {error}"));
    // Four inventories fail; the fifth connection recovers and then ends; the sixth
    // recovers and stays.
    let mut script = vec![Inventory::Fail; FAILED_INVENTORIES];
    script.extend([Inventory::RecoverThenClose, Inventory::RecoverAndStay]);
    let connections = script.len();
    let (accepted, mut accepted_at) = mpsc::unbounded_channel();
    let server = tokio::spawn(serve(listener, script, accepted));

    // Paused Tokio time only makes the waits instant. While the observer's journal writes
    // run, a paused runtime jumps ahead, so connection times cannot show the backoff; the
    // restart loop reports each wait it chose.
    tokio::time::pause();
    runtime
        .backend_ready(
            "2026-09-05T12:00:00Z"
                .to_owned()
                .try_into()
                .unwrap_or_else(|error| panic!("time: {error}")),
            Some(codex_router_host::BackendSchemaEvidence {
                running_executable: &identity,
                export: &export,
            }),
        )
        .await
        .unwrap_or_else(|error| panic!("backend ready: {error}"));
    for _ in 0..connections {
        accepted_at
            .recv()
            .await
            .unwrap_or_else(|| panic!("the observer connected again"));
    }
    tokio::time::resume();
    runtime
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown: {error}"));
    server
        .await
        .unwrap_or_else(|error| panic!("scripted app-server: {error}"));

    let reports = reports
        .lock()
        .unwrap_or_else(|error| panic!("reports: {error}"));
    assert_eq!(
        reports.delays,
        [1, 2, 4, 8, 1].map(Duration::from_secs),
        "failed attempts double the wait; the recovered observer's end starts again at 1 s"
    );
    let never_recovered = reports
        .warnings
        .iter()
        .filter(|message| message.starts_with(NEVER_RECOVERED))
        .count();
    assert_eq!(never_recovered, 1, "one warning for the run of failures");
}
