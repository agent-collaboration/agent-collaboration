//! A Router-side close (an agent without session/close) ends the session
//! NotLoaded like any close (the owner's close decision, design note §6):
//! a released session loads again, a release during a running prompt waits
//! for its turn, and releasing a session that isn't loaded answers nothing
//! to close. Through the runtime's public API; the load-path races at the
//! loading function stay unit tests beside it.

use super::provider_acp_message_fifo_support::{provider_binding, recorded_requests, target};
use super::provider_fixture_launch;
use codex_router_host::ExternalProviderRuntime;
use collaboration_protocol::{OperationId, SessionId};
use std::sync::Arc;

/// Oracle: the owner's close decision (ACP close: it ends NotLoaded and can
/// be loaded again; the Router-side close is no longer terminal). Released on
/// an agent that can load but can't close, the session loads again on the
/// same connection, and the agent sees both session/loads.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_released_session_loads_again_on_the_same_connection() {
    // Arrange: a loaded, then released, session on a load-capable agent
    // without session/close.
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.json");
    let runtime = ExternalProviderRuntime::initialize(
        provider_fixture_launch::launch(
            "cursor-settings-agent.ts",
            vec!["reactive".to_owned(), receipt.display().to_string()],
        ),
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");
    runtime
        .load_session("reactive-session-1".to_owned(), root.path().to_owned())
        .await
        .expect("the first load");
    runtime
        .release_session("reactive-session-1".to_owned())
        .await
        .expect("the Router releases the session");

    // Act
    let reloaded = runtime
        .load_session("reactive-session-1".to_owned(), root.path().to_owned())
        .await;
    let loads = recorded_requests(&receipt, "session/load");
    runtime.shutdown().await;

    // Assert
    assert!(
        reloaded.is_ok(),
        "a released session loads again: {reloaded:?}"
    );
    assert_eq!(loads, 2, "the agent saw both session/loads");
}

/// Oracle: design note §12.1 (a close that finds a turn running frees the
/// session only after that turn ends) and the owner's close decision (it
/// ends NotLoaded and can be loaded again). On an agent that can load but
/// can't close, a release during a running prompt interrupts it, waits for
/// its end, frees the session, and the session loads again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_release_during_a_running_prompt_frees_the_session_for_a_new_load() {
    // Arrange: a created session with a held prompt running.
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.json");
    let mut target = target();
    target.session_id = SessionId::try_from("reactive-session-1".to_owned()).expect("session ID");
    let binding = provider_binding(&target);
    let runtime = Arc::new(
        ExternalProviderRuntime::initialize(
            provider_fixture_launch::launch(
                "cursor-settings-agent.ts",
                vec![
                    "reactive".to_owned(),
                    receipt.display().to_string(),
                    "hold-first-prompt-open".to_owned(),
                ],
            ),
            Arc::new(acp_provider_adapters::StandardAcpBehavior),
        )
        .await
        .expect("fixture provider"),
    );
    runtime
        .create_session(root.path().to_owned())
        .await
        .expect("session/new");
    let held_context = codex_router_host::ExternalProviderApprovalContext {
        requester: target.clone().into(),
        approver: target.clone().into(),
        target: target.clone(),
        operation_id: OperationId::generate(),
        binding_generation: binding.generation.clone(),
        binding_retirement: runtime.retirement(),
    };
    let held = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        async move {
            runtime
                .prompt_with_approval_context(
                    "reactive-session-1".to_owned(),
                    "first".to_owned(),
                    held_context,
                )
                .await
        }
    });
    while recorded_requests(&receipt, "session/prompt") < 1 {
        tokio::task::yield_now().await;
    }

    // Act
    let released = runtime
        .release_session("reactive-session-1".to_owned())
        .await;
    let held = held.await.expect("held prompt task");
    let reloaded = runtime
        .load_session("reactive-session-1".to_owned(), root.path().to_owned())
        .await;
    let cancels = recorded_requests(&receipt, "session/cancel");
    let loads = recorded_requests(&receipt, "session/load");
    runtime.shutdown().await;

    // Assert
    assert!(released.is_ok(), "the release completes: {released:?}");
    assert!(held.is_ok(), "the interrupted prompt returns: {held:?}");
    assert_eq!(cancels, 1, "the running turn was interrupted once");
    assert!(
        reloaded.is_ok(),
        "the freed session loads again: {reloaded:?}"
    );
    assert_eq!(loads, 1);
}

/// Oracle: the owner's close decision and M8: close on a session that isn't
/// loaded is refused "nothing to close", with the state and its valid next
/// steps; the agent sees no request for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn releasing_a_session_that_is_not_loaded_answers_nothing_to_close() {
    // Arrange: a load-capable agent without session/close, nothing loaded.
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.json");
    let runtime = ExternalProviderRuntime::initialize(
        provider_fixture_launch::launch(
            "cursor-settings-agent.ts",
            vec!["reactive".to_owned(), receipt.display().to_string()],
        ),
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");

    // Act
    let released = runtime
        .release_session("reactive-session-1".to_owned())
        .await;
    let loads = recorded_requests(&receipt, "session/load");
    runtime.shutdown().await;

    // Assert
    assert!(
        matches!(
            &released,
            Err(acp_client_runtime::ExternalProviderRuntimeError::SessionStateRefused { answer })
                if answer == "session is not loaded; nothing to close; load it or resume it"
        ),
        "the release answers nothing to close: {released:?}"
    );
    assert_eq!(loads, 0, "the agent saw no session/load");
}
