//! The Router's work roots reach a session it starts in auto (D1 R18, V33):
//! Claude's session/new carries them as the Router's per-session settings
//! layer with its hook (the Router's Claude entry adds it to Claude's
//! settings); Cursor's carries them as ACP additional directories. A
//! session started in a named mode gets no work roots.

#![expect(
    clippy::expect_used,
    reason = "A Host or fixture that cannot start or answer must fail its test."
)]

use crate::provider_conversation_submission::submit_provider_create;
use crate::provider_send_behaviour::ComposedHost;
use codex_router_host::ExternalProviderLaunchBinding;
use collaboration_protocol::{
    ConversationCreateRequest, OperationId, ProviderRequestedPolicy, ProviderWorkingDirectory,
    RouterAccess,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn frames(receipt: &Path) -> Vec<Value> {
    std::fs::read(receipt)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .unwrap_or_default()
}

/// Oracle: D1 R18 and V33, "Claude's settings carry the roots and the hook"
/// (wrong build: "the hook missing"): a Claude session started in auto on a
/// binding with work roots sends session/new with the Router's work-root
/// settings (_meta.codexRouter.workRootSettings) naming each existing root as an
/// additional directory and the Router's installed hook, run under the
/// launch's Node, on every write and the shell.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_claude_session_in_auto_gets_the_work_roots_and_the_hook() {
    // Arrange
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let work_root = staging.path().join("work-root");
    std::fs::create_dir_all(&work_root).expect("work root");
    let binding = ExternalProviderLaunchBinding::claude(
        node_test_fixture::node_executable().expect("Node"),
        vec![
            node_test_fixture::fixture_program("provider-steering-agent.ts")
                .to_string_lossy()
                .into_owned(),
            "send-race".to_owned(),
            receipt.display().to_string(),
        ],
    )
    .expect("Claude binding")
    .with_work_roots(vec![work_root.clone()]);
    let host = ComposedHost::start(binding, "claude-local").await;
    let owner = ComposedHost::human("work-roots-owner");

    // Act
    let _target = host.create(&owner).await;
    let installed_hook = host.root.path().join("hooks/router-work-roots-hook.ts");
    let hook_installed = installed_hook.is_file();
    host.runtime.shutdown().await.expect("Host shutdown");

    // Assert
    let session_new = frames(&receipt)
        .into_iter()
        .find(|frame| frame["method"] == "session/new")
        .expect("session/new reached the agent");
    let settings = &session_new["params"]["_meta"]["codexRouter"]["workRootSettings"];
    let directories = settings["permissions"]["additionalDirectories"]
        .as_array()
        .expect("additional directories")
        .iter()
        .filter_map(Value::as_str)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    assert!(directories.contains(&work_root), "{directories:?}");
    let hook = &settings["hooks"]["PreToolUse"][0];
    assert_eq!(hook["matcher"], "Write|Edit|MultiEdit|NotebookEdit|Bash");
    let command = hook["hooks"][0]["command"].as_str().expect("hook command");
    assert!(hook_installed, "the hook is installed under the Host root");
    assert!(
        command.contains(&installed_hook.display().to_string())
            && command.contains(&work_root.display().to_string()),
        "the command runs the installed hook over the session's roots: {command}"
    );
}

/// Starts a Cursor session on the permission-mode fixture in `mode` with one
/// configured work root, and `named` roots from its creator; returns the
/// configured root and the additional directories session/new carried.
async fn cursor_session_directories(mode: &str, named: &[PathBuf]) -> (PathBuf, Vec<PathBuf>) {
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("receipt.json");
    let work_root = staging.path().join("work-root");
    std::fs::create_dir_all(&work_root).expect("work root");
    let binding = ExternalProviderLaunchBinding::cursor(
        node_test_fixture::node_executable().expect("Node"),
        vec![
            node_test_fixture::fixture_program("permission-mode-agent.ts")
                .to_string_lossy()
                .into_owned(),
            receipt.display().to_string(),
            "cursor".to_owned(),
            "grants-directories".to_owned(),
        ],
    )
    .expect("Cursor binding")
    .with_work_roots(vec![work_root.clone()]);
    let host = ComposedHost::start(binding, "cursor-local").await;
    let owner = ComposedHost::human("work-roots-owner");
    let create = OperationId::generate();
    submit_provider_create(
        &host.client,
        ConversationCreateRequest {
            work_roots: named
                .iter()
                .map(|root| {
                    ProviderWorkingDirectory::try_from(root.display().to_string())
                        .expect("named root")
                })
                .collect(),
            settings: Some(collaboration_protocol::ProviderRequestedSettings {
                mode: Some(mode.into()),
                ..collaboration_protocol::ProviderRequestedSettings::default()
            }),
            operation_id: create.clone(),
            endpoint: host.endpoint.clone(),
            generation: None,
            working_directory: ProviderWorkingDirectory::try_from(
                host.root.path().display().to_string(),
            )
            .expect("working directory"),
            created_by: owner.clone(),
            approver: owner,
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
        },
    )
    .await
    .expect("create admitted");
    // The fixture records session/new before it answers, so the settled
    // create has its working set on record.
    host.client
        .wait_for_provider_conversation_operation(
            collaboration_protocol::ConversationOperationWaitRequest {
                operation_id: create,
                timeout_seconds: collaboration_protocol::PositiveSeconds::try_from(10)
                    .expect("bound"),
            },
        )
        .await
        .expect("create settles");
    let recorded: Value =
        serde_json::from_slice(&std::fs::read(&receipt).expect("fixture receipt"))
            .expect("receipt JSON");
    let session = recorded
        .get("sessions")
        .and_then(Value::as_array)
        .and_then(|sessions| sessions.first())
        .expect("session/new was recorded");
    let directories = session["additionalDirectories"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default();
    host.runtime.shutdown().await.expect("Host shutdown");
    (work_root, directories)
}

/// Oracle: D1 R18, "Cursor: allowance only, plus ACP additionalDirectories
/// where it's accepted": a Cursor session started in auto is told the work
/// root as an additional directory.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cursor_session_in_auto_gets_the_work_roots_as_additional_directories() {
    // Arrange, Act
    let (work_root, directories) = cursor_session_directories("auto", &[]).await;

    // Assert
    assert!(directories.contains(&work_root), "{directories:?}");
}

/// Oracle: D1 R18 applies to "a Router-started session in auto" only, and
/// R5's named mode runs "with ... the allowance off": a Cursor session
/// started in a named mode is not told the work roots.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_in_a_named_mode_gets_no_work_roots() {
    // Arrange, Act
    let (work_root, directories) = cursor_session_directories("agent", &[]).await;

    // Assert
    assert!(!directories.contains(&work_root), "{directories:?}");
}

/// Oracle: D1 R18's list, "plus any roots the creator names at start": a
/// Cursor session started in auto with a root its creator names is told that
/// root too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_root_the_creator_names_joins_the_sessions_roots() {
    // Arrange
    let named_folder = tempfile::tempdir().expect("named root");
    let named = named_folder.path().to_path_buf();

    // Act
    let (_configured, directories) =
        cursor_session_directories("auto", std::slice::from_ref(&named)).await;

    // Assert
    assert!(directories.contains(&named), "{directories:?}");
}

fn claude_send_race(receipt: &Path) -> ExternalProviderLaunchBinding {
    ExternalProviderLaunchBinding::claude(
        node_test_fixture::node_executable().expect("Node"),
        vec![
            node_test_fixture::fixture_program("provider-steering-agent.ts")
                .to_string_lossy()
                .into_owned(),
            "send-race".to_owned(),
            receipt.display().to_string(),
        ],
    )
    .expect("Claude binding")
}

/// Oracle: D1 R18 and the Reviewer's D3, "a missing hook must fail closed":
/// when the Host can't install the hook (its hooks folder is a file), a Claude
/// start in auto is refused naming why, and nothing starts at the agent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn claude_in_auto_is_refused_when_its_hook_cannot_be_installed() {
    // Arrange: the Host root's hooks location is taken by a file.
    let root = tempfile::tempdir().expect("runtime root");
    std::fs::set_permissions(
        root.path(),
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
    )
    .expect("private runtime root");
    std::fs::write(root.path().join("hooks"), b"not a folder").expect("block the hooks folder");
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = ComposedHost::start_in(root, claude_send_race(&receipt), "claude-local").await;
    let owner = ComposedHost::human("work-roots-owner");

    // Act
    let refused = submit_provider_create(
        &host.client,
        ConversationCreateRequest {
            settings: Some(collaboration_protocol::ProviderRequestedSettings {
                mode: Some("auto".into()),
                ..collaboration_protocol::ProviderRequestedSettings::default()
            }),
            operation_id: OperationId::generate(),
            endpoint: host.endpoint.clone(),
            generation: None,
            working_directory: ProviderWorkingDirectory::try_from(
                host.root.path().display().to_string(),
            )
            .expect("working directory"),
            created_by: owner.clone(),
            approver: owner,
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            work_roots: Vec::new(),
        },
    )
    .await;
    let session_news = frames(&receipt)
        .iter()
        .filter(|frame| frame["method"] == "session/new")
        .count();
    host.runtime.shutdown().await.expect("Host shutdown");

    // Assert
    let refusal = format!("{refused:?}");
    assert!(refused.is_err(), "the auto start is refused: {refusal}");
    assert!(
        refusal.contains("work-roots hook isn't available"),
        "the refusal names the missing hook: {refusal}"
    );
    assert_eq!(session_news, 0, "nothing starts at the agent");
}

/// Oracle: the Reviewer's D3, "install the boundary independently of whether
/// the configurable roots list is empty": a Claude session in auto on a
/// binding with no configured roots still carries the hook over its own
/// folders.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn claude_in_auto_carries_the_hook_with_an_empty_roots_list() {
    // Arrange
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = ComposedHost::start(claude_send_race(&receipt), "claude-local").await;
    let owner = ComposedHost::human("work-roots-owner");

    // Act
    let _target = host.create(&owner).await;
    host.runtime.shutdown().await.expect("Host shutdown");

    // Assert
    let session_new = frames(&receipt)
        .into_iter()
        .find(|frame| frame["method"] == "session/new")
        .expect("session/new reached the agent");
    let layer = &session_new["params"]["_meta"]["codexRouter"];
    assert_eq!(layer["startMode"], "auto");
    let command = layer["workRootSettings"]["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .expect("the hook command");
    assert!(
        command.contains("router-work-roots-hook.ts"),
        "the installed hook runs: {command}"
    );
}
