//! A settlement whose durable write fails (design note §12.5, S5): the
//! operation's one completion owner keeps the outcome it observed as a live
//! overlay, wait answers that outcome and the durable stage in one line,
//! the row stays where it was, and nothing is sent again. The write fails
//! through the store's test-support StoreFault. That the overlay outlives
//! later settlements is a private invariant, proven by the supervisor's
//! sibling unit test.

use super::provider_acp_message_fifo_support::{provider_binding, recorded_requests, target};
use super::provider_fixture_launch;
use codex_router_host::{
    ExternalProviderBinding, ExternalProviderRuntime, ExternalProviderSupervisor,
};
use collaboration_contracts::ProviderConversationBackend;
use collaboration_protocol::{
    ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationShowRequest, ConversationOperationWaitRequest, ConversationPromptRequest,
    MessageContent, MessageDelivery, MessageText, OperationId, PositiveSeconds,
    ProviderOperationEffect, ProviderOperationStage, ProviderRequestedPolicy,
    ProviderWorkingDirectory, RouterAccess, SessionId,
};
use collaboration_runtime::{ProviderOperationStore, ProviderSessionRecord};
use std::sync::Arc;
use tokio::sync::Mutex;

const UNSAVED_APPLIED: &str =
    "the provider effect was applied; the saved result isn't available and may differ";

/// Oracle: §12.5, written by hand: show and wait never claim a blanket
/// "applied"; the row stays MayHaveDispatched; no retry and no second send.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_settlement_that_fails_to_save_answers_its_outcome_and_durable_stage() {
    // Arrange
    let root = tempfile::tempdir().expect("fixture root");
    let receipt = root.path().join("wire.json");
    let mut target = target();
    target.session_id = SessionId::try_from("reactive-session-1".to_owned()).expect("session");
    let runtime = ExternalProviderRuntime::initialize(
        provider_fixture_launch::launch(
            "cursor-settings-agent.ts",
            vec!["reactive".to_owned(), receipt.display().to_string()],
        ),
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture provider");
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from(
                root.path().display().to_string(),
            )
            .expect("working directory"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: target.clone().into(),
            approver: target.clone().into(),
            updated_at_ms: 1,
            resume: Default::default(),
        })
        .await
        .expect("session record");
    let supervisor = Arc::new(
        ExternalProviderSupervisor::new(
            vec![ExternalProviderBinding {
                identity: provider_binding(&target),
                runtime,
            }],
            Arc::clone(&store),
        )
        .expect("supervisor"),
    );
    let created = supervisor
        .open_provider_fixture(&target.endpoint, root.path().to_owned())
        .await
        .expect("session/new");
    assert_eq!(created, String::from(target.session_id.clone()));
    let operation_id = OperationId::generate();
    store
        .lock()
        .await
        .fail_next_terminal_write_of(&operation_id);

    // Act: the agent answers; the terminal write fails.
    supervisor
        .prompt(ConversationPromptRequest {
            delivery: MessageDelivery::Auto,
            operation_id: operation_id.clone(),
            input_id: None,
            target: target.clone(),
            generation: None,
            requested_by: target.clone().into(),
            approver: target.clone().into(),
            prompt: MessageContent::HumanUser {
                text: MessageText::try_from("unsaved".to_owned()).expect("prompt"),
            },
        })
        .await
        .expect("sent");
    let waited = supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id: operation_id.clone(),
            timeout_seconds: PositiveSeconds::try_from(10).expect("bound"),
        })
        .await;
    let shown = supervisor
        .show(ConversationOperationShowRequest {
            operation_id: operation_id.clone(),
        })
        .await
        .expect("shown");
    let row = store
        .lock()
        .await
        .inspect(&operation_id)
        .await
        .expect("row read")
        .expect("row");
    let prompts = recorded_requests(&receipt, "session/prompt");
    let stopped = supervisor.shutdown().await;

    // Assert
    let unsaved = |waited: &Result<_, ConversationOperationFailure>| match waited {
        Err(failure) => Some((
            failure.kind,
            failure.effect,
            String::from(failure.message.clone()),
        )),
        Ok(_) => None,
    };
    assert_eq!(
        unsaved(&waited),
        Some((
            ConversationOperationFailureKind::Unavailable,
            ProviderOperationEffect::Applied,
            UNSAVED_APPLIED.to_owned(),
        )),
        "{waited:?}"
    );
    assert_eq!(
        row.stage,
        ProviderOperationStage::MayHaveDispatched,
        "no retry wrote the row"
    );
    assert_eq!(
        shown.stage,
        ProviderOperationStage::MayHaveDispatched,
        "show reports the durable stage, never a blanket applied"
    );
    assert_eq!(prompts, 1, "nothing was sent again");
    assert_eq!(stopped, Ok(()));
}
