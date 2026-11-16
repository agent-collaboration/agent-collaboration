//! Close on an agent that offers no session/close (D1 R14, owner: "close
//! should just interrupt the agent and close", every agent): the Router does
//! the close itself.

use super::lifecycle_host::{
    LifecycleHost, SHORT_WAIT, conversation_failure, cursor_reactive, human, prompt_texts,
    recorded, until_recorded,
};
use super::{completed, not_sent_reason};
use collaboration_protocol::{
    ConversationOperationSettlement, ConversationOperationWaitOutput, OperationId,
    ProviderOperationEffect, ProviderPromptStopReason,
};

/// Oracle: D1 R14 and the Lead's 16 Nov ruling from the Verifier's live V27:
/// with a held turn and two queued prompts on an agent with no session/close,
/// close is admitted. Both queued prompts end not sent, "session closed";
/// the turn is interrupted (Cancelled); the result says the Router closed the
/// session itself; the agent is never asked to close; and a later prompt is
/// refused as closed, with nothing more reaching the agent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_on_an_agent_without_session_close_interrupts_and_closes_it_here() {
    // Arrange: a held turn, a head and one buffered entry, on an agent that
    // doesn't advertise session/close.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = LifecycleHost::start(cursor_reactive(&receipt, &["hold-first-prompt-open"])).await;
    let owner = human("router-close-owner");
    let target = host.create(&owner).await;
    let held = OperationId::generate();
    let _held = host.spawn_prompt(&target, &owner, held.clone(), "first", SHORT_WAIT);
    until_recorded(&receipt, "session/prompt", 1).await;
    let queued = host
        .queue_in_order(
            &target,
            &owner,
            &["queued-0".to_owned(), "queued-1".to_owned()],
        )
        .await;

    // Act: close mid-turn, then try to send again.
    let close = OperationId::generate();
    let close_admitted = host.close(&target, &owner, close.clone()).await;
    // Asserted where it's read: a refused close would leave the held turn
    // running and every later wait hanging.
    assert!(
        close_admitted.is_ok(),
        "close is admitted on an agent without session/close: {close_admitted:?}"
    );
    let closed = host.wait(&close).await;
    let held_settled = completed(&host.wait(&held).await);
    let mut settled = Vec::new();
    for operation_id in &queued {
        settled.push(host.wait(operation_id).await);
    }
    let later = host
        .prompt(
            &target,
            &owner,
            OperationId::generate(),
            "later",
            SHORT_WAIT,
        )
        .await;
    let prompts = prompt_texts(&receipt);
    let cancels = recorded(&receipt, "session/cancel");
    let agent_closes = recorded(&receipt, "session/close");
    let (_root, stopped) = host.stop().await;

    // Assert
    assert!(
        matches!(
            closed,
            Ok(collaboration_protocol::ConversationOperationWaitResult {
                output: ConversationOperationWaitOutput::Available {
                    settlement: ConversationOperationSettlement::Closed {
                        router_closed: true,
                        ..
                    }
                },
                ..
            })
        ),
        "the Router closed the session itself: {closed:?}"
    );
    assert_eq!(held_settled, Some(ProviderPromptStopReason::Cancelled));
    assert_eq!(cancels, 1, "the running turn is interrupted once");
    assert_eq!(agent_closes, 0, "the agent is never asked to close");
    for waited in &settled {
        assert_eq!(
            not_sent_reason(waited),
            Some((
                "not sent: session closed".to_owned(),
                ProviderOperationEffect::None
            )),
            "a queued prompt ends not sent: {waited:?}"
        );
    }
    let refusal = match &later {
        Err(collaboration_client::ConversationClientError::Client(error)) => {
            conversation_failure(error)
        }
        other => panic!("a later prompt is refused: {other:?}"),
    };
    assert_eq!(
        String::from(refusal.message),
        "the session is closed"
    );
    assert_eq!(
        prompts,
        vec!["first".to_owned()],
        "nothing more reached the agent"
    );
    stopped.expect("Host shutdown");
}
