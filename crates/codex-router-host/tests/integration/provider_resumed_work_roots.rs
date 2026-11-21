//! A session the Router created in auto gets its work roots back when it is
//! loaded after a Host restart (D1 R18; owner Decision 3, option A): the load
//! sends the work-root layer its create sent, rebuilt from the session's
//! record in the reopened store, and show reports the mode its create asked
//! for.

#![expect(
    clippy::expect_used,
    reason = "A Host or fixture that cannot start or answer must fail its test."
)]

use crate::provider_conversation_submission::submit_provider_load;
use crate::provider_send_behaviour::ComposedHost;
use codex_router_host::ExternalProviderLaunchBinding;
use collaboration_client::ConversationOperationResult;
use collaboration_protocol::{
    ConversationLoadRequest, OperationId, ProviderRequestedPolicy, ProviderSessionInspectRequest,
    ProviderWorkingDirectory, RouterAccess,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The Claude binding for the send-race fixture, which offers session/load,
/// with one configured work root.
fn claude_with_load(receipt: &Path, work_root: &Path) -> ExternalProviderLaunchBinding {
    ExternalProviderLaunchBinding::claude(
        node_test_fixture::node_executable().expect("Node"),
        vec![
            node_test_fixture::fixture_program("provider-steering-agent.ts")
                .to_string_lossy()
                .into_owned(),
            "send-race".to_owned(),
            receipt.display().to_string(),
            "replay-on-load".to_owned(),
        ],
    )
    .expect("Claude binding")
    .with_work_roots(vec![work_root.to_path_buf()])
}

/// The Router's layer (`_meta.codexRouter`) on the first `method` request the
/// agent recorded.
fn router_layer(receipt: &Path, method: &str) -> Value {
    let frames: Vec<Value> =
        serde_json::from_slice(&std::fs::read(receipt).expect("fixture receipt"))
            .expect("fixture frames");
    frames
        .into_iter()
        .find(|frame| frame.get("method").and_then(Value::as_str) == Some(method))
        .and_then(|frame| frame.pointer("/params/_meta/codexRouter").cloned())
        .expect("the request reached the agent with the Router's layer")
}

/// Oracle: owner Decision 3, "write at create, read at resume to rebuild the
/// roots and the mode"; D1 R18, a session in auto works in the Router's work
/// roots and the roots its creator named (wrong build: the load sends no
/// layer, drops the named root, or show forgets the mode). After the Host
/// stops and reopens the same root, the session's load carries the same
/// work-root layer its session/new carried, naming the configured and the
/// named root, and show reports the requested mode auto.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_loaded_after_a_restart_gets_its_created_work_roots_back() {
    // Arrange: a session created in auto naming a root, then a Host restart.
    let staging = tempfile::tempdir().expect("receipt folder");
    let created_receipt = staging.path().join("created.json");
    let loaded_receipt = staging.path().join("loaded.json");
    let work_root = staging.path().join("work-root");
    let named_root = staging.path().join("named-root");
    std::fs::create_dir_all(&work_root).expect("work root");
    std::fs::create_dir_all(&named_root).expect("named root");
    let host = ComposedHost::start(
        claude_with_load(&created_receipt, &work_root),
        "claude-local",
    )
    .await;
    let owner = ComposedHost::human("resume-owner");
    let target = host
        .create_naming(
            &owner,
            vec![
                ProviderWorkingDirectory::try_from(named_root.display().to_string())
                    .expect("named root"),
            ],
        )
        .await;
    host.runtime.shutdown().await.expect("Host shutdown");
    let ComposedHost { root, .. } = host;
    let host = ComposedHost::start_in(
        root,
        claude_with_load(&loaded_receipt, &work_root),
        "claude-local",
    )
    .await;

    // Act
    let loaded = submit_provider_load(
        &host.client,
        ConversationLoadRequest {
            operation_id: OperationId::generate(),
            target: target.clone(),
            generation: None,
            working_directory: ProviderWorkingDirectory::try_from(
                host.root.path().display().to_string(),
            )
            .expect("working directory"),
            requested_by: owner.clone(),
            approver: owner.clone(),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
        },
    )
    .await;
    let shown = host
        .client
        .inspect_provider_session(ProviderSessionInspectRequest {
            target: target.clone(),
        })
        .await;
    host.runtime
        .shutdown()
        .await
        .expect("restarted Host shutdown");

    // Assert
    assert!(
        matches!(loaded, Ok(ConversationOperationResult::Completed { .. })),
        "the load completes: {loaded:?}"
    );
    let created = router_layer(&created_receipt, "session/new");
    let restored = router_layer(&loaded_receipt, "session/load");
    assert_eq!(restored["startMode"], "auto", "{restored}");
    let directories = restored["workRootSettings"]["permissions"]["additionalDirectories"]
        .as_array()
        .expect("the restored layer names its directories")
        .iter()
        .filter_map(Value::as_str)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    assert!(
        directories.contains(&work_root) && directories.contains(&named_root),
        "the configured and the named root come back: {directories:?}"
    );
    assert_eq!(
        restored, created,
        "the load carries the layer its create did"
    );
    let shown = shown.expect("show answers");
    assert_eq!(shown.requested_mode.as_deref(), Some("auto"), "{shown:?}");
}
