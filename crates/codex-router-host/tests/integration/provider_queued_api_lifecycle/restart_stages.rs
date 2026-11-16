//! What a waiting API prompt's record says after the Router stops (D1 spec
//! R14, Lead decision 2026-11-16): a clean shutdown settles it terminal, not
//! sent; a hard stop leaves it admitted, never sent. Each case reopens the same
//! root and asserts the exact stage.

use super::lifecycle_host::{
    HostRoot, LifecycleHost, SHORT_WAIT, conversation_failure, cursor_reactive, human,
    private_root, prompt_texts, recorded, release, until_recorded,
};
use codex_router_host::{ExternalProviderLaunchBinding, HostCoordinationPaths, HostInstance};
use collaboration_protocol::{OperationId, ProviderOperationEffect, ProviderOperationStage};
use std::path::{Path, PathBuf};

/// The endpoint the send-race steering fixture serves.
const CLAUDE_ENDPOINT: &str = "claude-local";
/// The exit code a hard-stopped child returns once its prompt waits in the line.
const HARD_STOP_EXIT: i32 = 72;
const CHILD_ROOT: &str = "D1_R14_CHILD_ROOT";
const CHILD_RECEIPT: &str = "D1_R14_CHILD_RECEIPT";
const CHILD_RECORD: &str = "D1_R14_CHILD_RECORD";

/// The Claude binding for the send-race steering fixture with `behaviours`.
#[expect(
    clippy::expect_used,
    reason = "A fixture that cannot launch must fail its test."
)]
fn claude_send_race(receipt: &Path, behaviours: &[&str]) -> ExternalProviderLaunchBinding {
    let mut arguments = vec![
        node_test_fixture::fixture_program("provider-steering-agent.ts")
            .to_string_lossy()
            .into_owned(),
        "send-race".to_owned(),
        receipt.display().to_string(),
    ];
    arguments.extend(behaviours.iter().map(|behaviour| (*behaviour).to_owned()));
    ExternalProviderLaunchBinding::claude(
        node_test_fixture::node_executable().expect("Node"),
        arguments,
    )
    .expect("Claude binding")
}

/// Oracle: D1 spec R14 — "If ... the Router shuts down before it runs, it
/// says not sent with the reason": after a clean shutdown the waiting API
/// prompt's record is Terminal with effect None, and a same-ID retry on the
/// restarted Host returns that record and sends nothing. (The reason text is
/// on the waiter's settlement, not the stored row.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_api_prompt_at_clean_shutdown_ends_not_sent() {
    // Arrange: a held turn and one queued prompt, then a clean shutdown.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = LifecycleHost::start(cursor_reactive(&receipt, &["hold-first-prompt"])).await;
    let owner = human("clean-stop-owner");
    let target = host.create(&owner).await;
    let _held = host.spawn_prompt(
        &target,
        &owner,
        OperationId::generate(),
        "first",
        SHORT_WAIT,
    );
    until_recorded(&receipt, "session/prompt", 1).await;
    let queued = host
        .queue_in_order(&target, &owner, &["queued".to_owned()])
        .await
        .first()
        .expect("one queued prompt")
        .clone();
    let (root, stopped) = host.stop().await;
    let restarted_receipt = staging.path().join("restarted.json");
    let host = LifecycleHost::start_in(root, cursor_reactive(&restarted_receipt, &[])).await;

    // Act
    let shown = host.show(&queued).await;
    let _loaded = host.load(&target, &owner).await;
    let retry = host
        .prompt(&target, &owner, queued.clone(), "queued", SHORT_WAIT)
        .await;
    let prompts_after_restart = recorded(&restarted_receipt, "session/prompt");
    let (_root, restarted_stopped) = host.stop().await;

    // Assert
    stopped.expect("the clean shutdown has no operation-drain timeout");
    let shown = shown.expect("the queued prompt is on record after restart");
    assert_eq!(shown.stage, ProviderOperationStage::Terminal, "{shown:?}");
    assert_eq!(shown.effect, ProviderOperationEffect::None, "{shown:?}");
    assert_eq!(shown.terminal_stop_reason, None, "it never ran: {shown:?}");
    let retry = match &retry {
        Err(collaboration_client::ConversationClientError::Client(error)) => {
            conversation_failure(error)
        }
        other => panic!("a same-ID retry returns the settled record: {other:?}"),
    };
    assert_eq!(retry.operation_id, Some(queued));
    assert_eq!(retry.effect, ProviderOperationEffect::None);
    assert_eq!(prompts_after_restart, 0, "a same-ID retry sends nothing");
    restarted_stopped.expect("restarted Host shutdown");
}

/// Oracle: D1 spec R14 and V27 — "a Router restart drops what's waiting. A
/// CLI or tool prompt that was waiting then shows as 'admitted, never sent',
/// and sending it again with the same request ID returns that same record
/// rather than sending it". The prompt first meets the race (the steer finds
/// no turn, the one re-decision's prompt meets a running turn), so its row was
/// marked and unmarked before it queued; then the Host stops hard (the
/// process exits, no shutdown runs). On the reopened root its row is exactly
/// Admitted with effect None.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_api_prompt_after_restart_shows_admitted_never_sent() {
    // Arrange: a child test process runs the Host and exits hard mid-queue.
    let root = private_root();
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let record = staging.path().join("child-record.json");
    let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .arg("--exact")
        .arg(child_selector(
            "hard_stop_child_queues_one_prompt_then_exits",
        ))
        .arg("--nocapture")
        .env(CHILD_ROOT, root.path())
        .env(CHILD_RECEIPT, &receipt)
        .env(CHILD_RECORD, &record)
        .status()
        .expect("child test runs");
    // Exit 72 is the child's own signal; a selector that matched nothing exits 0.
    assert_eq!(
        status.code(),
        Some(HARD_STOP_EXIT),
        "the child reached its hard stop"
    );
    // The stopped Host's agent still holds its turn; let it end so it exits.
    release(&receipt);
    let child: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&record).expect("child record")).expect("JSON");
    let queued = OperationId::try_from(
        child
            .get("queued")
            .and_then(serde_json::Value::as_str)
            .expect("queued ID")
            .to_owned(),
    )
    .expect("queued operation ID");
    let target: collaboration_protocol::SessionRef =
        serde_json::from_value(child.get("target").cloned().expect("target")).expect("target");
    let owner = human("hard-stop-owner");
    let restarted_receipt = staging.path().join("restarted.json");

    // Act: reopen the same root as a restarted Host does: the singleton first
    // clears the endpoint sockets the stopped process left, then the Host starts.
    HostInstance::acquire(HostCoordinationPaths::new(
        root.path().join("operator.sock"),
        root.path().join("host.lock"),
    ))
    .expect("singleton")
    .clear_stale_endpoint_sockets(root.path())
    .await
    .expect("stale endpoint sockets cleared");
    let host = LifecycleHost::start_on(
        HostRoot::Owned(root),
        claude_send_race(&restarted_receipt, &[]),
        CLAUDE_ENDPOINT,
    )
    .await;
    let shown = host.show(&queued).await;
    let _loaded = host.load(&target, &owner).await;
    let retry = host
        .prompt(&target, &owner, queued.clone(), "second", SHORT_WAIT)
        .await;
    let prompts_after_restart = recorded(&restarted_receipt, "session/prompt");
    let steers_after_restart = recorded(&restarted_receipt, "_session/steering");
    let (_root, restarted_stopped) = host.stop().await;

    // Assert
    assert_eq!(
        child
            .get("stage_before_stop")
            .and_then(serde_json::Value::as_str),
        Some("admitted"),
        "the refused send was unmarked before it queued: {child}"
    );
    assert_eq!(
        child
            .get("steers_before_stop")
            .and_then(serde_json::Value::as_u64),
        Some(1),
        "it met the race: one steer, answered promptRequired, before it queued: {child}"
    );
    assert_eq!(
        child.get("texts"),
        Some(&serde_json::json!(["first"])),
        "only the held turn was prompted before the stop: {child}"
    );
    let shown = shown.expect("the queued prompt is on record after restart");
    assert_eq!(shown.stage, ProviderOperationStage::Admitted, "{shown:?}");
    assert_eq!(shown.effect, ProviderOperationEffect::None, "{shown:?}");
    assert!(
        !matches!(
            &retry,
            Ok(collaboration_client::ConversationOperationResult::Completed { .. })
        ),
        "a same-ID retry does not run it: {retry:?}"
    );
    assert_eq!(prompts_after_restart, 0, "a same-ID retry sends nothing");
    assert_eq!(steers_after_restart, 0, "and steers nothing");
    restarted_stopped.expect("restarted Host shutdown");
}

/// The child half of the hard-stop case: only runs when the parent set its
/// root; it queues one prompt through the race, records it, and exits 72.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hard_stop_child_queues_one_prompt_then_exits() {
    let Ok(root) = std::env::var(CHILD_ROOT) else {
        return;
    };
    let receipt = PathBuf::from(std::env::var(CHILD_RECEIPT).expect("child receipt"));
    let record = PathBuf::from(std::env::var(CHILD_RECORD).expect("child record"));
    let host = LifecycleHost::start_on(
        HostRoot::Borrowed(PathBuf::from(root)),
        claude_send_race(&receipt, &["first-steer-before-turn-end"]),
        CLAUDE_ENDPOINT,
    )
    .await;
    let owner = human("hard-stop-owner");
    let target = host.create(&owner).await;
    let _running = host.spawn_prompt(
        &target,
        &owner,
        OperationId::generate(),
        "first",
        SHORT_WAIT,
    );
    until_recorded(&receipt, "session/prompt", 1).await;
    let queued = OperationId::generate();
    let _second = host.spawn_prompt(&target, &owner, queued.clone(), "second", SHORT_WAIT);
    host.until_router_queued(&queued).await;
    let shown = host.show(&queued).await.expect("queued prompt on record");
    let stage = match shown.stage {
        ProviderOperationStage::Admitted => "admitted",
        ProviderOperationStage::MayHaveDispatched => "mayHaveDispatched",
        ProviderOperationStage::Terminal => "terminal",
    };
    std::fs::write(
        &record,
        serde_json::json!({"queued": queued.as_str(), "target": target, "stage_before_stop": stage, "steers_before_stop": recorded(&receipt, "_session/steering"), "texts": prompt_texts(&receipt)})
            .to_string(),
    )
    .expect("child record");
    // A hard stop while the turn still runs: no shutdown, no destructors.
    std::process::exit(HARD_STOP_EXIT);
}

/// The child's exact selector, derived from this module's path.
fn child_selector(case: &str) -> String {
    let module = module_path!();
    let relative = module.split_once("::").map_or(module, |(_, path)| path);
    format!("{relative}::{case}")
}
