//! Only a restore the connection admits opens a new run of a session (D1 R14,
//! the Reviewer's G1): a competing attempt the connection refuses leaves the
//! live run's activation and its close alone, and a completion that
//! outlived its run publishes and registers nothing.

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
    hub_runtime_with(&[]).await
}

/// The Cursor fixture with these behaviours, on a real hub.
async fn hub_runtime_with(behaviours: &[&str]) -> HubRuntime {
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
            ["reactive".to_owned(), receipt.display().to_string()]
                .into_iter()
                .chain(behaviours.iter().map(|behaviour| (*behaviour).to_owned()))
                .collect(),
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
        matches!(
            competing,
            Err(ExternalProviderRuntimeError::LocalBusy { .. })
        ),
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
            Err(ExternalProviderRuntimeError::LocalBusy { .. })
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

/// The hub's state for `session_id` on the fixture's endpoint.
async fn hub_state(fixture: &HubRuntime, session_id: &str) -> Result<SessionState, String> {
    let mut target = fixture.hub_target.clone();
    target.session_id =
        message_board::SessionId::try_from(session_id.to_owned()).expect("session ID");
    SessionEventHub::state(fixture.hub.as_ref(), target)
        .await
        .map_err(|error| format!("{error:?}"))
}

/// Oracle: the Reviewer's G1 follow-up. A session is loaded and active; a
/// create whose session/new answers that same identity is rejected as a
/// duplicate. The rejected create opens no run, so the loaded run's close
/// still publishes Closed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rejected_duplicate_create_leaves_the_live_run_alone() {
    // Arrange: reactive-session-2 loaded; the fixture's next session/new
    // answers that same identity.
    let fixture = hub_runtime().await;
    let runtime = Arc::clone(&fixture.runtime);
    let cwd = fixture.receipt.parent().expect("root").to_owned();
    let loaded = runtime
        .load_session("reactive-session-2".to_owned(), cwd.clone())
        .await;

    // Act: the duplicate create; then the loaded run closes; a load of
    // another session flushes the hub.
    let duplicate = runtime.create_session(cwd.clone()).await;
    let closed = runtime
        .release_session("reactive-session-2".to_owned())
        .await;
    let flushed = runtime
        .load_session("reactive-session-7".to_owned(), cwd)
        .await;
    let state_after_close = hub_state(&fixture, "reactive-session-2").await;
    runtime.shutdown().await;

    // Assert
    assert!(loaded.is_ok(), "{loaded:?}");
    assert!(
        matches!(&duplicate, Err(ExternalProviderRuntimeError::Operation(message)) if message.contains("duplicate")),
        "the duplicate identity is rejected: {duplicate:?}"
    );
    assert!(closed.is_ok(), "{closed:?}");
    assert!(flushed.is_ok(), "{flushed:?}");
    assert!(
        matches!(state_after_close, Ok(SessionState::Closed)),
        "the loaded run's close published Closed: {state_after_close:?}"
    );
}

/// Oracle: the Reviewer's G2 follow-up. A create's continuation is held after
/// the client's reply; the agent closes the session natively; the session is
/// loaded again as a new run, which starts a turn. The old create then goes
/// on: it publishes nothing into the new run (the hub stays Running) and
/// answers closed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_create_continuation_held_past_a_close_publishes_nothing_into_the_next_run() {
    // Arrange: a create held before its activation, on an agent that closes.
    let fixture = hub_runtime_with(&["advertise-close", "hold-first-prompt-open"]).await;
    let runtime = Arc::clone(&fixture.runtime);
    let cwd = fixture.receipt.parent().expect("root").to_owned();
    let (activation_reached, activation_release) = runtime.pause_next_activation();
    let create = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        let cwd = cwd.clone();
        async move { runtime.create_session(cwd).await }
    });
    activation_reached
        .await
        .expect("the create's client answered and it is held");

    // Act: native close; the session is loaded as a new run that starts a
    // turn; then the old create goes on.
    let closed = runtime.close_session("reactive-session-1".to_owned()).await;
    let reloaded = runtime
        .load_session("reactive-session-1".to_owned(), cwd)
        .await;
    let turn = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        async move {
            runtime
                .prompt("reactive-session-1".to_owned(), "hold".to_owned())
                .await
        }
    });
    while recorded_requests(&fixture.receipt, "session/prompt") < 1 {
        tokio::task::yield_now().await;
    }
    let mut running = SessionEventHub::attach(fixture.hub.as_ref(), fixture.hub_target.clone())
        .await
        .expect("hub attachment");
    while !matches!(
        hub_state(&fixture, "reactive-session-1").await,
        Ok(SessionState::Running)
    ) {
        let _event = running.receiver.recv().await.expect("hub event");
    }
    let _go = activation_release.send(());
    let created = create.await.expect("create task");
    let state_after_old_create = hub_state(&fixture, "reactive-session-1").await;
    node_test_fixture::release_fixture(&fixture.receipt, ".release").expect("release the turn");
    let turn = turn.await.expect("turn task");
    runtime.shutdown().await;

    // Assert
    assert!(closed.is_ok(), "{closed:?}");
    assert!(reloaded.is_ok(), "{reloaded:?}");
    assert!(
        matches!(created, Err(ExternalProviderRuntimeError::SessionReleased)),
        "the old create answers closed: {created:?}"
    );
    assert!(
        matches!(state_after_old_create, Ok(SessionState::Running)),
        "the new run is still running: {state_after_old_create:?}"
    );
    assert!(turn.is_ok(), "{turn:?}");
}

/// Oracle: the Reviewer's G3. A load's reply is held; the agent closes the
/// session natively; then the old reply lands. It registers no actor, and a
/// fresh load of the session is admitted and activates.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restore_completing_after_a_native_close_registers_nothing() {
    // Arrange: a load whose reply waits, on an agent that closes.
    let fixture = hub_runtime_with(&["advertise-close", "hold-load"]).await;
    let runtime = Arc::clone(&fixture.runtime);
    let cwd = fixture.receipt.parent().expect("root").to_owned();
    let held = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        let cwd = cwd.clone();
        async move {
            runtime
                .load_session("reactive-session-1".to_owned(), cwd)
                .await
        }
    });
    while recorded_requests(&fixture.receipt, "session/load") < 1 {
        tokio::task::yield_now().await;
    }

    // Act: native close; the old reply lands; a fresh load.
    let closed = runtime.close_session("reactive-session-1".to_owned()).await;
    node_test_fixture::release_fixture(&fixture.receipt, ".release-load")
        .expect("release the load");
    let late = held.await.expect("held load task");
    let activity_after_late = runtime
        .session_activity("reactive-session-1".to_owned())
        .await;
    let fresh = runtime
        .load_session("reactive-session-1".to_owned(), cwd)
        .await;
    let state_after_fresh = hub_state(&fixture, "reactive-session-1").await;
    runtime.shutdown().await;

    // Assert
    assert!(closed.is_ok(), "{closed:?}");
    assert!(
        matches!(late, Err(ExternalProviderRuntimeError::SessionReleased)),
        "the late completion answers closed: {late:?}"
    );
    assert!(
        matches!(
            activity_after_late,
            Ok(crate::ProviderSessionActivity::NotLoaded)
        ),
        "the late completion registered no actor: {activity_after_late:?}"
    );
    assert!(fresh.is_ok(), "a fresh load is admitted: {fresh:?}");
    assert!(
        matches!(state_after_fresh, Ok(SessionState::Idle)),
        "the fresh run activated: {state_after_fresh:?}"
    );
}
