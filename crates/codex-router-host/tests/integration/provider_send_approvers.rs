//! Provider send consent and unknown-outcome fencing through the composed Host.
//! Each Host runs one fixture whose receipt records every frame it reads.

#![expect(
    clippy::expect_used,
    reason = "A Host or fixture that cannot start or answer must fail its test."
)]

use codex_router_host::ExternalProviderLaunchBinding;
use collaboration_client::{
    CollaborationAccess, ConversationActor, ConversationClient, ConversationClientError,
    ConversationOperationResult, ConversationPromptInput, ConversationSettlementDetail,
};
use collaboration_protocol::{
    ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationSettlement, ConversationOperationShowRequest,
    ConversationOperationSnapshot, ConversationOperationWaitOutput,
    ConversationOperationWaitRequest, OperationId, PositiveSeconds, ProviderIdentity,
    ProviderOperationEffect, ProviderOperationStage, SessionRef,
};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::provider_send_behaviour::{
    ComposedHost, SHORT_WAIT, cursor_reactive, recorded, until_recorded,
};

/// How long a prompt call waits for its settlement before answering with what
/// it knows: a product argument. The runner bounds each case.
const SETTLE_WAIT: Duration = Duration::from_secs(10);

type PromptAnswer = Result<ConversationOperationResult, ConversationClientError>;

/// One API prompt by `requester`, naming `approver`, answered after `wait`.
async fn send(
    host: &ComposedHost,
    target: &SessionRef,
    requester: &ProviderIdentity,
    approver: &ProviderIdentity,
    operation_id: &OperationId,
    text: &str,
    wait: Duration,
) -> PromptAnswer {
    send_with_delivery(
        host,
        target,
        (requester, approver),
        operation_id,
        text,
        wait,
        collaboration_protocol::MessageDelivery::Auto,
    )
    .await
}

/// As `send`, with the caller's delivery choice (D1 R12).
async fn send_with_delivery(
    host: &ComposedHost,
    target: &SessionRef,
    (requester, approver): (&ProviderIdentity, &ProviderIdentity),
    operation_id: &OperationId,
    text: &str,
    wait: Duration,
    delivery: collaboration_protocol::MessageDelivery,
) -> PromptAnswer {
    let conversation = ConversationClient::connect(
        &CollaborationAccess::api(host.client.directory()),
        &target.endpoint,
    )
    .await?;
    let input = ConversationPromptInput {
        delivery,
        operation_id: Some(operation_id.clone()),
        target: target.clone(),
        working_directory: None,
        requested_by: ConversationActor::Typed(requester.to_board_identity().expect("requester")),
        approver: Some(approver.clone()),
        message: serde_json::from_value(serde_json::json!({"kind":"humanUser","text":text}))
            .expect("prompt content"),
        effort: None,
        generation: None,
    };
    conversation
        .prompt(input, wait, CancellationToken::new())
        .await
}

/// The typed failure a refused or failed prompt answered with.
fn answered_failure(answer: &PromptAnswer) -> Option<ConversationOperationFailure> {
    match answer {
        Err(ConversationClientError::Client(error)) => error.conversation_failure(),
        _ => None,
    }
}

async fn show(
    host: &ComposedHost,
    operation_id: &OperationId,
) -> Option<ConversationOperationSnapshot> {
    host.client
        .show_provider_conversation_operation(ConversationOperationShowRequest {
            operation_id: operation_id.clone(),
        })
        .await
        .ok()
}

/// The session's events on the Host's hub, as (kind, inputId) pairs (V22,
/// Lead 2026-11-16: the IDs live inside the Router, never on the ACP wire).
async fn settled(
    host: &ComposedHost,
    operation_id: &OperationId,
) -> Result<ConversationOperationSettlement, collaboration_client::ClientError> {
    let request = ConversationOperationWaitRequest {
        operation_id: operation_id.clone(),
        timeout_seconds: PositiveSeconds::try_from(10).expect("timeout"),
    };
    loop {
        let waited = host
            .client
            .wait_for_provider_conversation_operation(request.clone())
            .await?;
        if let ConversationOperationWaitOutput::Available { settlement } = waited.output {
            return Ok(settlement);
        }
    }
}

/// Every frame the fixture read.
fn frames(receipt: &Path) -> Vec<Value> {
    std::fs::read(receipt)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .unwrap_or_default()
}

/// The frames with `method` whose first prompt block's text is `text`.
fn carrying(receipt: &Path, method: &str, text: &str) -> usize {
    frames(receipt)
        .iter()
        .filter(|frame| frame.get("method").and_then(Value::as_str) == Some(method))
        .filter(|frame| {
            frame
                .pointer("/params/prompt/0/text")
                .and_then(Value::as_str)
                == Some(text)
        })
        .count()
}

/// The text of every prompt the fixture read, in order.
fn release(receipt: &Path) {
    node_test_fixture::release_fixture(receipt, ".release").expect("release the held turn");
}

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

/// Oracle: D1 V22 (R12, C2) — "a same-process unknown failure whose completed
/// entry is still retained in live_operations, followed at once by a new
/// send", with "zero steer and zero prompt at the fixture and the
/// uncertainty refusal". The unknown failure is a steer the agent answered
/// with an outcome no client knows; it also ended the turn, so the new send
/// finds the session idle.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_right_after_an_unknown_steer_is_refused() {
    // Arrange: a held turn, and a steer into it whose outcome is unknown.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = ComposedHost::start(
        claude_send_race(&receipt, &["first-steer-unknown"]),
        "claude-local",
    )
    .await;
    let owner = ComposedHost::human("race-owner");
    let target = host.create(&owner).await;
    let first = OperationId::generate();
    let held = send(&host, &target, &owner, &owner, &first, "first", SHORT_WAIT).await;
    until_recorded(&receipt, "session/prompt", 1).await;
    let unknown = OperationId::generate();
    let unknown_answer = send(
        &host,
        &target,
        &owner,
        &owner,
        &unknown,
        "second",
        SETTLE_WAIT,
    )
    .await;
    let unknown_shown = show(&host, &unknown).await;

    // Act: a new send at once.
    let third = OperationId::generate();
    let refused = send(&host, &target, &owner, &owner, &third, "third", SETTLE_WAIT).await;
    let third_shown = show(&host, &third).await;
    let steers = carrying(&receipt, "_session/steering", "third");
    let prompts = carrying(&receipt, "session/prompt", "third");
    let all_prompts = recorded(&receipt, "session/prompt");
    host.runtime.shutdown().await.expect("Host shutdown");

    // Assert: the steer's outcome is unknown, and the new send is refused
    // for it without reaching the agent.
    assert!(
        matches!(held, Ok(ConversationOperationResult::Pending { .. })),
        "the first turn is held: {held:?}"
    );
    assert!(
        answered_failure(&unknown_answer)
            .is_some_and(|failure| failure.effect == ProviderOperationEffect::Unknown),
        "the steer's outcome is unknown: {unknown_answer:?}"
    );
    assert!(
        unknown_shown.as_ref().is_some_and(|snapshot| {
            snapshot.stage == ProviderOperationStage::Terminal
                && snapshot.effect == ProviderOperationEffect::Unknown
        }),
        "its row is terminal Unknown: {unknown_shown:?}"
    );
    let refusal = answered_failure(&refused)
        .unwrap_or_else(|| panic!("the new send is refused: {refused:?}"));
    assert_eq!(
        refusal.kind,
        ConversationOperationFailureKind::Busy,
        "{refusal:?}"
    );
    assert_eq!(
        String::from(refusal.message.clone()),
        format!(
            "an earlier prompt's outcome is unknown; check the session, then run operation reconcile for {}",
            unknown.as_str()
        ),
        "the refusal names the unknown operation"
    );
    assert_eq!(refusal.effect, ProviderOperationEffect::None);
    assert!(
        third_shown.is_none(),
        "a refused send leaves no row: {third_shown:?}"
    );
    assert_eq!(steers, 0, "zero steer");
    assert_eq!(prompts, 0, "zero prompt");
    assert_eq!(all_prompts, 1, "only the held turn's prompt");
}

/// Oracle: D1 V22 (R12, C2) — "a same-process unknown failure whose completed
/// entry is still retained in live_operations ... The same three run at
/// queued dispatch, where the entry ends not sent." A message queues behind
/// the running turn; a later steer into that turn ends unknown; when the turn
/// ends, the queued entry names the earlier operation that needs reconciliation,
/// and the agent receives no steer or prompt for it after the release.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_entry_after_an_unknown_steer_ends_not_sent() {
    // Arrange: a held turn; the first steer finds the turn "ended" while it
    // still runs, so that message waits in the line; the second steer's
    // outcome is unknown.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = ComposedHost::start(
        claude_send_race(
            &receipt,
            &["first-steer-before-turn-end", "second-steer-unknown"],
        ),
        "claude-local",
    )
    .await;
    let owner = ComposedHost::human("race-owner");
    let target = host.create(&owner).await;
    let first = OperationId::generate();
    let held = send(&host, &target, &owner, &owner, &first, "first", SHORT_WAIT).await;
    until_recorded(&receipt, "session/prompt", 1).await;
    let waiting = OperationId::generate();
    let queued = send(
        &host, &target, &owner, &owner, &waiting, "queued", SHORT_WAIT,
    )
    .await;
    let unknown = OperationId::generate();
    let unknown_answer = send(
        &host,
        &target,
        &owner,
        &owner,
        &unknown,
        "unknown",
        SETTLE_WAIT,
    )
    .await;
    let unknown_shown = show(&host, &unknown).await;
    let steers_before = recorded(&receipt, "_session/steering");
    let prompts_before = recorded(&receipt, "session/prompt");

    // Act: the turn ends and the line dispatches its entry.
    release(&receipt);
    let waiting_settled = settled(&host, &waiting).await;
    let waiting_shown = show(&host, &waiting).await;
    let first_settled = settled(&host, &first).await;
    let steers_after = recorded(&receipt, "_session/steering");
    let prompts_after = recorded(&receipt, "session/prompt");
    let queued_prompts = carrying(&receipt, "session/prompt", "queued");
    host.runtime.shutdown().await.expect("Host shutdown");

    // Assert: the entry ends not sent, and nothing more reached the agent.
    assert!(
        matches!(held, Ok(ConversationOperationResult::Pending { .. })),
        "the first turn is held: {held:?}"
    );
    assert!(
        matches!(&queued, Ok(ConversationOperationResult::Queued { operation_id, .. }) if operation_id == &waiting),
        "the message waits in the line: {queued:?}"
    );
    assert!(
        answered_failure(&unknown_answer)
            .is_some_and(|failure| failure.effect == ProviderOperationEffect::Unknown),
        "the later steer's outcome is unknown: {unknown_answer:?}"
    );
    assert!(
        unknown_shown.as_ref().is_some_and(|snapshot| {
            snapshot.stage == ProviderOperationStage::Terminal
                && snapshot.effect == ProviderOperationEffect::Unknown
        }),
        "its row is terminal Unknown: {unknown_shown:?}"
    );
    let not_sent = waiting_settled
        .as_ref()
        .err()
        .and_then(collaboration_client::ClientError::conversation_failure)
        .unwrap_or_else(|| panic!("the entry ends not sent: {waiting_settled:?}"));
    assert_eq!(
        String::from(not_sent.message.clone()),
        format!(
            "an earlier prompt's outcome is unknown; check the session, then run operation reconcile for {}",
            unknown.as_str()
        ),
        "{not_sent:?}"
    );
    assert_eq!(not_sent.effect, ProviderOperationEffect::None);
    assert!(
        waiting_shown.as_ref().is_some_and(|snapshot| {
            snapshot.stage == ProviderOperationStage::Terminal
                && snapshot.effect == ProviderOperationEffect::None
        }),
        "{waiting_shown:?}"
    );
    assert!(
        matches!(
            first_settled,
            Ok(ConversationOperationSettlement::PromptCompleted { .. })
        ),
        "the held turn ran to its end: {first_settled:?}"
    );
    assert_eq!(steers_after, steers_before, "zero steer at dispatch");
    assert_eq!(prompts_after, prompts_before, "zero prompt at dispatch");
    assert_eq!(queued_prompts, 0, "the waiting message was never prompted");
}

/// Oracle: D1 spec R12 and C1, review F3: "the result names the running
/// turn's approver when it differs from the requested one", for a turn that
/// mail started too. Mail starts a turn on a session approved by A; an API
/// prompt naming B is steered into it, and its result names A.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn steer_into_a_mail_started_turn_names_its_approver() {
    // Arrange: a session approved by A, whose agent holds its first turn and
    // steers while it runs; mail starts that turn.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = ComposedHost::start(
        cursor_reactive(&receipt, &["hold-first-prompt-open", "advertise-steering"]),
        "cursor-local",
    )
    .await;
    let approver_a = ComposedHost::human("approver-a");
    let approver_b = ComposedHost::human("approver-b");
    let target = host.create(&approver_a).await;
    let mailed = crate::session_message_route_composition::route_fixture::deliver(
        &host.runtime,
        target.clone(),
    )
    .await;
    until_recorded(&receipt, "session/prompt", 1).await;

    // Act: an API prompt naming B, while the mail-started turn runs.
    let second = OperationId::generate();
    let steered = send(
        &host,
        &target,
        &approver_b,
        &approver_b,
        &second,
        "second",
        SETTLE_WAIT,
    )
    .await;
    release(&receipt);
    host.runtime.shutdown().await.expect("Host shutdown");

    // Assert: mail started the turn; the steer names that turn's approver.
    assert!(
        matches!(
            mailed.outcome,
            collaboration_protocol::DeliveryOutcome::Started
        ),
        "{mailed:?}"
    );
    match steered {
        Ok(ConversationOperationResult::Completed { settlement, .. }) => match settlement.detail {
            ConversationSettlementDetail::ProviderSteered { turn_approver } => {
                assert_eq!(
                    turn_approver,
                    Some(approver_a),
                    "the mail-started turn's approver"
                );
            }
            other => panic!("expected steered, got {other:?}"),
        },
        other => panic!("expected a steered completion, got {other:?}"),
    }
}
