//! `conversation cancel` on a prompt still waiting in its session's line takes
//! it out of the line (D1 R12's queued follow-up, "cancel it"): it ends not
//! sent and the running turn keeps running. A running prompt keeps the
//! interrupt.

use super::lifecycle_host::{
    LifecycleHost, SHORT_WAIT, cursor_reactive, human, prompt_texts, recorded, release,
    until_recorded,
};
use super::{completed, not_sent_reason};
use collaboration_protocol::{
    ConversationCancelRequest, ConversationOperationSettlement, ConversationOperationWaitOutput,
    OperationId, ProviderOperationEffect, ProviderPromptStopReason,
};

/// Oracle: D1 spec R12 ("queued, with ... a follow-up: how to wait on it,
/// cancel it or interrupt"), R14, and the Lead's 16 Nov decision: a
/// conversation cancel naming a queued prompt's operation settles the cancel,
/// ends that prompt not sent with no effect, never sends it, and doesn't
/// interrupt the running turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conversation_cancel_takes_a_queued_prompt_out_of_the_line() {
    // Arrange: a held turn and one queued prompt behind it.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = LifecycleHost::start(cursor_reactive(&receipt, &["hold-first-prompt-open"])).await;
    let owner = human("cancel-door-owner");
    let target = host.create(&owner).await;
    let held = OperationId::generate();
    let _held = host.spawn_prompt(&target, &owner, held.clone(), "first", SHORT_WAIT);
    until_recorded(&receipt, "session/prompt", 1).await;
    let queued = host
        .queue_in_order(&target, &owner, &["queued".to_owned()])
        .await;
    let waiting = queued.first().expect("the queued prompt").clone();

    // Act: cancel the queued prompt by its operation, then let the turn end.
    let cancel = OperationId::generate();
    let cancelled = crate::provider_conversation_submission::submit_provider_cancel(
        &host.client,
        ConversationCancelRequest {
            operation_id: cancel.clone(),
            target_operation_id: waiting.clone(),
            target: target.clone(),
            generation: None,
            requested_by: owner.clone(),
            approver: owner.clone(),
        },
    )
    .await;
    let cancel_settled = host.wait(&cancel).await;
    let waiter = host.wait_once(&waiting, 5).await;
    let cancels_at_agent = recorded(&receipt, "session/cancel");
    release(&receipt);
    let held_settled = completed(&host.wait(&held).await);
    let prompts = prompt_texts(&receipt);
    let (_root, stopped) = host.stop().await;

    // Assert
    assert!(cancelled.is_ok(), "the cancel is admitted: {cancelled:?}");
    assert!(
        matches!(
            cancel_settled,
            Ok(collaboration_protocol::ConversationOperationWaitResult {
                output: ConversationOperationWaitOutput::Available {
                    settlement: ConversationOperationSettlement::CancelRequested { .. }
                },
                ..
            })
        ),
        "the cancel settles: {cancel_settled:?}"
    );
    assert_eq!(
        not_sent_reason(&waiter),
        Some((
            "not sent: cancelled from the queue".to_owned(),
            ProviderOperationEffect::None
        )),
        "the queued prompt ends not sent: {waiter:?}"
    );
    assert_eq!(cancels_at_agent, 0, "the running turn is not interrupted");
    assert_eq!(held_settled, Some(ProviderPromptStopReason::EndTurn));
    assert_eq!(
        prompts,
        vec!["first".to_owned()],
        "the queued prompt never ran"
    );
    stopped.expect("Host shutdown without an operation-drain timeout");
}
