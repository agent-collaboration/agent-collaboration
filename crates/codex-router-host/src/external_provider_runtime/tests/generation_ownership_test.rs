//! Only a restore the connection admits opens a new run of a session (D1 R14,
//! the Reviewer's G1): a competing attempt the connection refuses leaves the
//! live run's activation and its close alone.

use super::*;
use collaboration_contracts::SessionEventHub;
use session_event_model::{SessionEvent, SessionState};

/// How many requests with `method` the Cursor fixture recorded in `receipt`.
fn recorded_requests(receipt: &std::path::Path, method: &str) -> usize {
    std::fs::read(receipt)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<serde_json::Value>>(&bytes).ok())
        .map_or(0, |requests| {
            requests
                .iter()
                .filter(|request| request["method"] == method)
                .count()
        })
}

struct HubRuntime {
    _root: tempfile::TempDir,
    receipt: PathBuf,
    hub: Arc<collaboration_runtime::ProviderSessionEventHub>,
    hub_target: message_board::SessionRef,
    runtime: Arc<ExternalProviderRuntime>,
}

async fn hub_runtime() -> HubRuntime {
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.json");
    let store = Arc::new(tokio::sync::Mutex::new(
        collaboration_runtime::ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    let hub = Arc::new(collaboration_runtime::ProviderSessionEventHub::new(store));
    let hub_target: message_board::SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint": {
            "serviceId": "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89",
            "endpointId": "cursor-local",
        },
        "sessionId": "reactive-session-1",
    }))
    .expect("hub target");
    let runtime = ExternalProviderRuntime::initialize_with_mcp_http_and_hub(
        super::acp_scripted_fixture::launch_fixture_agent(
            "cursor-settings-agent.ts",
            vec!["reactive".to_owned(), receipt.display().to_string()],
        ),
        acp_client_runtime::ProviderModelPicker::Standard,
        "fixture",
        "http://127.0.0.1:1/mcp",
        Arc::clone(&hub),
        hub_target.endpoint.clone(),
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");
    HubRuntime {
        receipt,
        hub,
        hub_target,
        runtime: Arc::new(runtime),
        _root: root,
    }
}

/// Oracle: the Reviewer's G1 and the Lead's order. Two loads of one session
/// race: the first is admitted and held after its client's success; the
/// second reaches the connection while the first's actor exists and is
/// refused LocalBusy. The first still activates: it returns Ok and the hub
/// shows Idle. Then a third, refused attempt doesn't stop the close that
/// follows from publishing Closed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_competing_load_leaves_the_admitted_run_alone() {
    // Arrange: a load of a session the agent knows, held before activation.
    let fixture = hub_runtime().await;
    let runtime = Arc::clone(&fixture.runtime);
    let cwd = fixture.receipt.parent().expect("root").to_owned();
    let (activation_reached, activation_release) = runtime.pause_next_activation();
    let first = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        let cwd = cwd.clone();
        async move {
            runtime
                .load_session("reactive-session-1".to_owned(), cwd)
                .await
        }
    });
    activation_reached
        .await
        .expect("the first load's client succeeded and it is held");

    // Act: a competing load is refused; the first goes on; another refused
    // attempt, then the session closes.
    let competing = runtime
        .load_session("reactive-session-1".to_owned(), cwd.clone())
        .await;
    let _go = activation_release.send(());
    let first = first.await.expect("first load task");
    // Checked here, so a refused attempt that ended the admitted run fails
    // at once rather than leaving the close below without its Closed.
    assert!(
        matches!(competing, Err(ExternalProviderRuntimeError::LocalBusy)),
        "the competing load is refused: {competing:?}"
    );
    assert!(first.is_ok(), "the admitted load activates: {first:?}");
    let state_after_first =
        SessionEventHub::state(fixture.hub.as_ref(), fixture.hub_target.clone()).await;
    let after_activation =
        SessionEventHub::attach(fixture.hub.as_ref(), fixture.hub_target.clone())
            .await
            .expect("hub attachment");
    let refused_before_close = runtime
        .load_session("reactive-session-1".to_owned(), cwd.clone())
        .await;
    let closed = runtime
        .release_session("reactive-session-1".to_owned())
        .await;
    // The connection enqueued the close's Closed before the release returned.
    // A new session's activation waits until the hub takes its Idle, so by
    // then the hub has applied everything enqueued before it.
    let flushed = runtime.create_session(cwd).await;
    let state_after_close =
        SessionEventHub::state(fixture.hub.as_ref(), fixture.hub_target.clone()).await;
    let loads = recorded_requests(&fixture.receipt, "session/load");
    runtime.shutdown().await;

    // Assert
    assert!(
        matches!(state_after_first, Ok(SessionState::Idle)),
        "the hub shows the admitted run idle: {state_after_first:?}"
    );
    assert!(
        after_activation.snapshot.iter().any(|event| matches!(
            event.event,
            SessionEvent::StateChanged {
                state: SessionState::Idle
            }
        )),
        "the admitted run published Idle: {:?}",
        after_activation.snapshot
    );
    assert!(
        matches!(
            refused_before_close,
            Err(ExternalProviderRuntimeError::LocalBusy)
        ),
        "{refused_before_close:?}"
    );
    assert!(closed.is_ok(), "the session closes: {closed:?}");
    assert!(flushed.is_ok(), "the flushing session started: {flushed:?}");
    assert!(
        matches!(state_after_close, Ok(SessionState::Closed)),
        "the close published Closed: {state_after_close:?}"
    );
    assert_eq!(loads, 1, "only the admitted load reached the agent");
}
