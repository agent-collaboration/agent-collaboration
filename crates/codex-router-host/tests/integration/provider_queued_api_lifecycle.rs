//! A queued API prompt's whole life through the real client → API →
//! supervisor path (D1 spec R12, R14; V26, V27): the line's bound, its
//! refusals, close, provider stop, queue cancel, Host restart and retention.
//! Each case starts a real Host with the Cursor ACP fixture, whose receipt
//! records every request the agent received.

mod lifecycle_host;
mod restart_stages;

use collaboration_client::{
    ConversationOperationResult, ConversationSettlementDetail, ProviderPromptOutput,
};
use collaboration_protocol::{
    ConversationOperationFailureKind, ConversationOperationQueueState,
    ConversationOperationSettlement, ConversationOperationWaitOutput,
    ConversationOutputUnavailableReason, OperationId, ProviderOperationEffect,
    ProviderOperationStage, ProviderPromptStopReason,
};
use lifecycle_host::{
    LifecycleHost, SHORT_WAIT, conversation_failure, cursor_reactive, human, prompt_texts,
    recorded, release, until_recorded,
};
use std::time::Duration;

/// A prompt call that waits long enough for an idle turn to settle.
const SETTLING_WAIT: Duration = Duration::from_secs(20);

/// The settlement of a prompt that ran to its end.
fn completed(
    waited: &Result<
        collaboration_protocol::ConversationOperationWaitResult,
        collaboration_client::ClientError,
    >,
) -> Option<ProviderPromptStopReason> {
    match waited {
        Ok(collaboration_protocol::ConversationOperationWaitResult {
            output:
                ConversationOperationWaitOutput::Available {
                    settlement: ConversationOperationSettlement::PromptCompleted { stop_reason, .. },
                },
            ..
        }) => Some(stop_reason.clone()),
        _ => None,
    }
}

/// The reason a waiter was told its prompt was not sent, from its failure.
fn not_sent_reason(
    waited: &Result<
        collaboration_protocol::ConversationOperationWaitResult,
        collaboration_client::ClientError,
    >,
) -> Option<(String, ProviderOperationEffect)> {
    let Err(error) = waited else { return None };
    let failure = conversation_failure(error);
    Some((String::from(failure.message), failure.effect))
}

fn texts(prefix: &str, count: usize) -> Vec<String> {
    (0..count)
        .map(|index| format!("{prefix}-{index}"))
        .collect()
}

/// Oracle: D1 spec R12 and V26 — "the line holds 128 waiting entries, the held
/// head included": with a turn held and the worker holding the first queued
/// entry, 127 more are accepted (128 waiting) and the next is refused "queue
/// full"; none is dropped or overwritten, and all 128 run in order after the
/// release.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queue_full_is_refused() {
    // Arrange: a held first turn, then 128 prompts queued in a known order.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = LifecycleHost::start(cursor_reactive(&receipt, &["hold-first-prompt"])).await;
    let owner = human("queue-owner");
    let target = host.create(&owner).await;
    let _held = host.spawn_prompt(
        &target,
        &owner,
        OperationId::generate(),
        "first",
        SHORT_WAIT,
    );
    until_recorded(&receipt, "session/prompt", 1).await;
    let queued_texts = texts("queued", 128);
    let queued = host.queue_in_order(&target, &owner, &queued_texts).await;

    // Act: one more prompt, then the held turn ends.
    let overflow = OperationId::generate();
    let refused = host
        .prompt(&target, &owner, overflow.clone(), "overflow", SHORT_WAIT)
        .await;
    let overflow_shown = host.show(&overflow).await;
    let prompts_while_held = recorded(&receipt, "session/prompt");
    release(&receipt);
    let mut settled = Vec::with_capacity(queued.len());
    for operation_id in &queued {
        settled.push(completed(&host.wait(operation_id).await));
    }
    let received = prompt_texts(&receipt);
    let (_root, stopped) = host.stop().await;

    // Assert
    let refusal = format!("{refused:?}");
    assert!(
        refused.is_err() && refusal.contains("queue full"),
        "the 129th waiting prompt is refused queue full: {refusal}"
    );
    let overflow_shown = overflow_shown.expect("the refused prompt is on record");
    assert_eq!(overflow_shown.stage, ProviderOperationStage::Terminal);
    assert_eq!(overflow_shown.effect, ProviderOperationEffect::None);
    assert_eq!(prompts_while_held, 1, "nothing is sent while the turn runs");
    assert!(
        settled
            .iter()
            .all(|stop| stop == &Some(ProviderPromptStopReason::EndTurn)),
        "every queued prompt ran to its end: {settled:?}"
    );
    let mut expected = vec!["first".to_owned()];
    expected.extend(queued_texts);
    assert_eq!(received, expected, "all 128 ran once each, in order");
    stopped.expect("Host shutdown");
}

/// Oracle: D1 spec R14, C5 and V26 — "a queued API prompt whose turn ends in a
/// permission refusal settles as a refusal, as an unqueued one does".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_api_prompt_permission_refusal_settles_as_refused() {
    // Arrange: a held turn and a queued prompt whose turn asks a permission
    // the Router refuses: the prompt names the session itself as approver,
    // and a session never approves its own request.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = LifecycleHost::start(cursor_reactive(
        &receipt,
        &["hold-first-prompt", "prompt-directives"],
    ))
    .await;
    let owner = human("permission-owner");
    let target = host.create(&owner).await;
    let _held = host.spawn_prompt(
        &target,
        &owner,
        OperationId::generate(),
        "first",
        SHORT_WAIT,
    );
    until_recorded(&receipt, "session/prompt", 1).await;
    let session_approver = collaboration_protocol::ProviderIdentity::Session(target.clone());
    let queued = host
        .queue_in_order_as(
            &target,
            &owner,
            &session_approver,
            &["ask-permission".to_owned()],
        )
        .await;
    let queued = queued.first().expect("one queued prompt").clone();

    // Act: the turn ends, the queued prompt runs; then the same prompt unqueued.
    release(&receipt);
    let queued_settled = host.wait(&queued).await;
    let unqueued = OperationId::generate();
    let unqueued_answer = host
        .prompt_as(
            &target,
            &owner,
            &session_approver,
            unqueued.clone(),
            "ask-permission",
            SETTLING_WAIT,
        )
        .await;
    let unqueued_settled = host.wait(&unqueued).await;
    let (_root, stopped) = host.stop().await;

    // Assert: both end as the same permission refusal, with the turn applied.
    let queued_failure = conversation_failure(
        queued_settled
            .as_ref()
            .err()
            .unwrap_or_else(|| panic!("the queued prompt settles refused: {queued_settled:?}")),
    );
    let unqueued_failure =
        conversation_failure(unqueued_settled.as_ref().err().unwrap_or_else(|| {
            panic!("the unqueued prompt settles refused: {unqueued_settled:?}")
        }));
    assert_eq!(
        queued_failure.kind,
        ConversationOperationFailureKind::PermissionRejected,
        "{queued_failure:?}"
    );
    assert_eq!(queued_failure.kind, unqueued_failure.kind);
    assert_eq!(queued_failure.effect, ProviderOperationEffect::Applied);
    assert_eq!(queued_failure.effect, unqueued_failure.effect);
    assert_eq!(queued_failure.message, unqueued_failure.message);
    assert!(unqueued_answer.is_err(), "{unqueued_answer:?}");
    stopped.expect("Host shutdown");
}

/// Oracle: D1 spec R14 (owner, 16 Nov: "close shoudl just interrupt the agent
/// and close") and V27 as replaced: with a held turn and two queued API
/// prompts, close is admitted mid-turn. Both queued prompts end not sent,
/// reason "session closed"; the running turn is interrupted (Cancelled); the
/// session is closed; no prompt reaches the agent after the held one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_during_a_running_turn_interrupts_it_and_ends_the_waiting_not_sent() {
    // Arrange: a held turn, a head and one buffered entry.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = LifecycleHost::start(cursor_reactive(
        &receipt,
        &["hold-first-prompt-open", "advertise-close"],
    ))
    .await;
    let owner = human("close-owner");
    let target = host.create(&owner).await;
    let held = OperationId::generate();
    let _held = host.spawn_prompt(&target, &owner, held.clone(), "first", SHORT_WAIT);
    until_recorded(&receipt, "session/prompt", 1).await;
    let queued_texts = texts("queued", 2);
    let queued = host.queue_in_order(&target, &owner, &queued_texts).await;

    // Act: close mid-turn.
    let close = OperationId::generate();
    let close_admitted = host.close(&target, &owner, close.clone()).await;
    // Asserted where it's read: a refused close would leave the held turn
    // running and every later wait hanging.
    assert!(
        close_admitted.is_ok(),
        "close is admitted mid-turn: {close_admitted:?}"
    );
    let closed = host.wait(&close).await;
    let held_settled = completed(&host.wait(&held).await);
    let mut settled = Vec::new();
    for operation_id in &queued {
        settled.push(host.wait(operation_id).await);
    }
    let prompts = prompt_texts(&receipt);
    let cancels = recorded(&receipt, "session/cancel");
    let closes = recorded(&receipt, "session/close");
    let (_root, stopped) = host.stop().await;

    // Assert
    assert!(
        matches!(
            closed,
            Ok(collaboration_protocol::ConversationOperationWaitResult {
                output: ConversationOperationWaitOutput::Available {
                    settlement: ConversationOperationSettlement::Closed { .. }
                },
                ..
            })
        ),
        "the session is closed: {closed:?}"
    );
    assert_eq!(held_settled, Some(ProviderPromptStopReason::Cancelled));
    assert_eq!(cancels, 1, "the running turn is interrupted once");
    assert_eq!(closes, 1, "the agent is asked to close once");
    for waited in &settled {
        let failure = conversation_failure(
            waited
                .as_ref()
                .err()
                .unwrap_or_else(|| panic!("a queued prompt ends not sent: {waited:?}")),
        );
        assert_eq!(
            String::from(failure.message.clone()),
            "not sent: session closed"
        );
        assert_eq!(failure.effect, ProviderOperationEffect::None);
    }
    assert_eq!(
        prompts,
        vec!["first".to_owned()],
        "no prompt reaches the agent after the held one"
    );
    stopped.expect("Host shutdown");
}

/// Oracle: D1 spec R14 and V27 — "the steered input is found as applied after
/// restart, and a same-ID retry injects nothing"; on the cold Host a
/// permission-refused prompt and a completed prompt with an unknown stop
/// reason are shown as applied, never as steered.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn steered_input_after_restart_is_found_and_not_reinjected() {
    // Arrange: a steered input, a permission-refused prompt and an
    // unknown-stop prompt on a warm Host, then a restart on the same root.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let behaviours = [
        "hold-first-prompt-open",
        "advertise-steering",
        "prompt-directives",
    ];
    let host = LifecycleHost::start(cursor_reactive(&receipt, &behaviours)).await;
    let owner = human("steer-owner");
    let target = host.create(&owner).await;
    let first = OperationId::generate();
    let _held = host.spawn_prompt(&target, &owner, first.clone(), "first", SHORT_WAIT);
    until_recorded(&receipt, "session/prompt", 1).await;
    let steered = OperationId::generate();
    let steered_warm = host
        .prompt(&target, &owner, steered.clone(), "steered", SETTLING_WAIT)
        .await;
    release(&receipt);
    let _first = host.wait(&first).await;
    let refused = OperationId::generate();
    let session_approver = collaboration_protocol::ProviderIdentity::Session(target.clone());
    let refused_warm = host
        .prompt_as(
            &target,
            &owner,
            &session_approver,
            refused.clone(),
            "ask-permission",
            SETTLING_WAIT,
        )
        .await;
    let unknown = OperationId::generate();
    let unknown_warm = host
        .prompt(
            &target,
            &owner,
            unknown.clone(),
            "unknown-stop",
            SETTLING_WAIT,
        )
        .await;
    let (root, stopped) = host.stop().await;
    stopped.expect("warm Host shutdown");
    let cold_receipt = staging.path().join("cold.json");
    let host = LifecycleHost::start_in(root, cursor_reactive(&cold_receipt, &behaviours)).await;

    // Act: show the steered input, load the session so a send would reach
    // the agent, and retry each under its own ID on the cold Host.
    let steered_shown = host.show(&steered).await;
    let loaded = host.load(&target, &owner).await;
    let mut cold = Vec::new();
    for (operation_id, approver, text) in [
        (&steered, &owner, "steered"),
        (&refused, &session_approver, "ask-permission"),
        (&unknown, &owner, "unknown-stop"),
    ] {
        cold.push(
            host.prompt_as(
                &target,
                &owner,
                approver,
                operation_id.clone(),
                text,
                SHORT_WAIT,
            )
            .await,
        );
    }
    let cold_steers = recorded(&cold_receipt, "_session/steering");
    let cold_prompts = recorded(&cold_receipt, "session/prompt");
    let cold_loads = recorded(&cold_receipt, "session/load");
    let (_root, cold_stopped) = host.stop().await;

    // Assert
    assert!(
        matches!(
            &steered_warm,
            Ok(ConversationOperationResult::Completed { settlement, .. })
                if matches!(settlement.detail, ConversationSettlementDetail::ProviderSteered { .. })
        ),
        "the warm input was steered: {steered_warm:?}"
    );
    let refused_kind = match &refused_warm {
        Err(collaboration_client::ConversationClientError::Client(error)) => {
            conversation_failure(error).kind
        }
        other => panic!("the warm prompt was refused a permission: {other:?}"),
    };
    assert_eq!(
        refused_kind,
        ConversationOperationFailureKind::PermissionRejected
    );
    assert!(
        matches!(
            &unknown_warm,
            Ok(ConversationOperationResult::Completed { settlement, .. })
                if matches!(settlement.stop_reason, Some(collaboration_client::ConversationStopReason::Unknown(_)))
        ),
        "the warm prompt completed with an unknown stop reason: {unknown_warm:?}"
    );
    assert!(
        loaded.is_ok(),
        "the cold Host loads the session: {loaded:?}"
    );
    assert_eq!(cold_loads, 1, "the agent has the session loaded");
    let steered_shown = steered_shown.expect("the steered input is on record");
    assert_eq!(steered_shown.stage, ProviderOperationStage::Terminal);
    assert_eq!(steered_shown.effect, ProviderOperationEffect::Applied);
    for answered in &cold {
        assert!(
            matches!(
                answered,
                Ok(ConversationOperationResult::Completed { settlement, .. })
                    if matches!(
                        &settlement.detail,
                        ConversationSettlementDetail::ProviderPrompt {
                            output: ProviderPromptOutput::Unavailable {
                                reason: ConversationOutputUnavailableReason::HostRestarted
                            }
                        }
                    )
            ),
            "shown as applied on the cold Host, never as steered: {answered:?}"
        );
    }
    assert_eq!(cold_steers, 0, "a same-ID retry injects nothing");
    assert_eq!(cold_prompts, 0, "a same-ID retry sends nothing");
    cold_stopped.expect("cold Host shutdown");
}

/// Oracle: D1 spec R14 and V27 — "a pruned ID reports no record ('the Router
/// has no record of this operation ID'), never 'never ran'". Retention runs on
/// the Host's one injected clock.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pruned_operation_is_reported_as_no_record() {
    // Arrange: a settled prompt on a Host whose clock the test owns, with a
    // one-day retention.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let root = lifecycle_host::private_root();
    let clock_listener =
        crate::host_clock_control::prepare(root.path()).expect("Host clock control input");
    let mut host = LifecycleHost::start_in(root, cursor_reactive(&receipt, &[])).await;
    let mut clock = crate::host_clock_control::HostClockControl::connected(clock_listener)
        .await
        .expect("Host clock control");
    let owner = human("retention-owner");
    let target = host.create(&owner).await;
    let pruned = OperationId::generate();
    let ran = host
        .prompt(&target, &owner, pruned.clone(), "settles", SETTLING_WAIT)
        .await;
    let shown_before = host.show(&pruned).await;
    host.runtime
        .configure_provider_operation_retention(std::num::NonZeroU32::new(1))
        .await
        .expect("retention configured");

    // Act: the injected clock passes the retention window, well past any
    // settlement stamp, and retention prunes the operation.
    clock
        .advance(Duration::from_secs(3_650 * 24 * 60 * 60))
        .await
        .expect("clock advanced");
    let shown_after = loop {
        match host.show(&pruned).await {
            Ok(_) => tokio::task::yield_now().await,
            Err(error) => break error,
        }
    };
    let (_root, stopped) = host.stop().await;

    // Assert
    assert!(ran.is_ok(), "{ran:?}");
    assert!(
        shown_before.is_ok(),
        "on record before retention: {shown_before:?}"
    );
    let failure = conversation_failure(&shown_after);
    assert_eq!(failure.kind, ConversationOperationFailureKind::NotFound);
    assert_eq!(
        String::from(failure.message.clone()),
        "the Router has no record of this operation ID"
    );
    assert!(
        !String::from(failure.message).contains("never ran"),
        "a pruned ID never reads as never ran"
    );
    stopped.expect("Host shutdown");
}

/// Oracle: D1 spec R14 and V27 — through the real API, with the worker
/// holding a head and more buffered, "queue cancel ... before dispatch
/// settles the waiter as not sent with the reason, with zero later sends, a
/// same-ID retry that doesn't replay, and a Host shutdown that completes
/// without an operation-drain timeout".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_cancel_settles_the_waiter_not_sent() {
    // Arrange: a held turn, a head and one buffered entry.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = LifecycleHost::start(cursor_reactive(&receipt, &["hold-first-prompt-open"])).await;
    let owner = human("cancel-owner");
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
        .queue_in_order(&target, &owner, &texts("queued", 2))
        .await;
    let buffered = queued.get(1).expect("buffered").clone();
    let input_id = host
        .show(&buffered)
        .await
        .expect("buffered entry on record")
        .input_id
        .expect("a queued prompt names its input");

    // Act: cancel the buffered entry, then let the turn end.
    let cancelled = host.queue_cancel(&target, input_id.as_str()).await;
    let waiter = host.wait_once(&buffered, 5).await;
    release(&receipt);
    let head = host.wait(queued.first().expect("head")).await;
    let retry = host
        .prompt(&target, &owner, buffered.clone(), "queued-1", SHORT_WAIT)
        .await;
    let prompts = prompt_texts(&receipt);
    let (_root, stopped) = host.stop().await;

    // Assert
    assert_eq!(cancelled["result"]["cancelled"], true, "{cancelled}");
    assert_eq!(
        not_sent_reason(&waiter),
        Some((
            "not sent: cancelled from the queue".to_owned(),
            ProviderOperationEffect::None
        )),
        "the waiter settles not sent: {waiter:?}; Host shutdown: {stopped:?}"
    );
    assert_eq!(completed(&head), Some(ProviderPromptStopReason::EndTurn));
    assert!(retry.is_err(), "a same-ID retry does not replay: {retry:?}");
    assert_eq!(prompts, vec!["first".to_owned(), "queued-0".to_owned()]);
    stopped.expect("Host shutdown without an operation-drain timeout");
}

/// Oracle: D1 spec R14 and V27 — through the real API, with the worker
/// holding a head and more buffered, "provider stop before dispatch settles
/// the waiter as not sent with the reason, with zero later sends, a same-ID
/// retry that doesn't replay, and a Host shutdown that completes without an
/// operation-drain timeout".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_stop_settles_queued_waiters_not_sent() {
    // Arrange: a held turn, a head and one buffered entry.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = LifecycleHost::start(cursor_reactive(
        &receipt,
        &["hold-first-prompt-open", "exit-on-stop-file"],
    ))
    .await;
    let owner = human("stop-owner");
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
        .queue_in_order(&target, &owner, &texts("queued", 2))
        .await;

    // Act: the agent stops.
    std::fs::write(staging.path().join("wire.json.stop"), b"").expect("stop the agent");
    let mut settled = Vec::new();
    for operation_id in &queued {
        settled.push(not_sent_reason(&host.wait_once(operation_id, 5).await));
    }
    let retry = host
        .prompt(
            &target,
            &owner,
            queued.first().expect("head").clone(),
            "queued-0",
            SHORT_WAIT,
        )
        .await;
    let prompts = prompt_texts(&receipt);
    let (_root, stopped) = host.stop().await;

    // Assert
    for reason in &settled {
        assert_eq!(
            reason,
            &Some((
                "not sent: providerRetired".to_owned(),
                ProviderOperationEffect::None
            )),
            "every waiter is not sent, with the reason: {settled:?}"
        );
    }
    assert!(retry.is_err(), "a same-ID retry does not replay: {retry:?}");
    assert_eq!(prompts, vec!["first".to_owned()], "zero later sends");
    stopped.expect("Host shutdown without an operation-drain timeout");
}
