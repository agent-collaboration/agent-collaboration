//! Who approves a send, and when an earlier unknown outcome refuses one,
//! through the real client → API → ConversationOperations → supervisor path
//! (D1 spec R12, C1, C2; V22). A steered message joins the running turn, so
//! that turn's approver keeps deciding its permissions; a queued message runs
//! as its own turn, approved by the approver it named. A send made while an
//! unknown outcome is unresolved, fresh or queued, reaches the agent never.
//! Each Host runs one fixture whose receipt records every frame it reads.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "A Host or fixture that cannot start or answer must fail its test."
)]

use codex_router_host::ExternalProviderLaunchBinding;
use collaboration_client::{
    CollaborationAccess, ConversationActor, ConversationClient, ConversationClientError,
    ConversationOperationResult, ConversationPromptInput, ConversationSettlementDetail,
};
use collaboration_protocol::{
    ApprovalDecideParams, ApprovalDetailedRecord, ApprovalOptionEffect, ApprovalOptionViewScope,
    ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationSettlement, ConversationOperationShowRequest,
    ConversationOperationSnapshot, ConversationOperationWaitOutput,
    ConversationOperationWaitRequest, OperationId, PositiveSeconds, ProviderIdentity,
    ProviderOperationEffect, ProviderOperationStage, SessionRef,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
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
async fn input_events(host: &ComposedHost, target: &SessionRef) -> Vec<(String, String)> {
    host.client
        .observe_provider_session(collaboration_protocol::BoundedObservationRequest {
            target: target.clone(),
            timeout_seconds: 1,
            max_events: 4096,
            max_bytes: 1_048_576,
            after_sequence: None,
            epoch: None,
        })
        .await
        .expect("provider observation")
        .events
        .iter()
        .filter_map(|record| record.get("event"))
        .filter_map(|event| {
            let kind = event.get("kind").and_then(Value::as_str)?;
            let input_id = event.get("inputId").and_then(Value::as_str)?;
            (kind == "inputAccepted" || kind == "turnStarted")
                .then(|| (kind.to_owned(), input_id.to_owned()))
        })
        .collect()
}

/// Waits on the Host until the operation settles; a settled failure is the
/// client's error.
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
fn prompt_texts(receipt: &Path) -> Vec<String> {
    frames(receipt)
        .iter()
        .filter(|frame| frame.get("method").and_then(Value::as_str) == Some("session/prompt"))
        .filter_map(|frame| {
            frame
                .pointer("/params/prompt/0/text")
                .and_then(Value::as_str)
        })
        .map(str::to_owned)
        .collect()
}

/// The client's answer to the fixture's permission request `request_id`.
fn permission_answer(receipt: &Path, request_id: &str) -> Option<Value> {
    frames(receipt).into_iter().find(|frame| {
        frame.get("method").is_none() && frame.get("id").and_then(Value::as_str) == Some(request_id)
    })
}

/// The first pending approval; fails at once if the fixture's permission
/// request was answered without one.
async fn pending_approval(
    host: &ComposedHost,
    receipt: &Path,
    request_id: &str,
) -> ApprovalDetailedRecord {
    loop {
        let pending = host
            .client
            .list_approvals_with_options(true)
            .await
            .expect("pending approvals")
            .approvals;
        if let Some(approval) = pending.into_iter().next() {
            return approval;
        }
        if let Some(answer) = permission_answer(receipt, request_id) {
            panic!("the permission was answered without an approval: {answer}");
        }
        tokio::task::yield_now().await;
    }
}

/// Decides `approval` with its allow-once option, as the approver it
/// reached, so the test observes whoever that was.
async fn allow_once_as_its_approver(host: &ComposedHost, approval: &ApprovalDetailedRecord) {
    let option = approval
        .options
        .iter()
        .find(|option| {
            option.effect == ApprovalOptionEffect::Allow
                && option.scope == ApprovalOptionViewScope::Once
        })
        .expect("an allow-once option");
    host.client
        .decide_approval(ApprovalDecideParams {
            request_id: approval.request_id.clone(),
            decision: None,
            option_id: Some(option.option_id.clone()),
            note: None,
            acknowledge_persistent: false,
            actor: approval.approver.clone(),
        })
        .await
        .expect("approval decision");
}

async fn until_answered(receipt: &Path, request_id: &str) -> Value {
    loop {
        if let Some(answer) = permission_answer(receipt, request_id) {
            return answer;
        }
        tokio::task::yield_now().await;
    }
}

/// Ends the fixture's held first turn.
fn release(receipt: &Path) {
    std::fs::write(PathBuf::from(format!("{}.release", receipt.display())), b"")
        .expect("release the held turn");
}

fn same_session(approval: &ApprovalDetailedRecord, target: &SessionRef) -> bool {
    serde_json::to_value(&approval.requester).ok() == serde_json::to_value(target).ok()
}

/// The send-race fixture as a Claude binding, recording to `receipt`.
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

/// Oracle: D1 V22 (R12, C1) — "the held turn is owned by approver A; the
/// second message names approver B. Steered: a permission request raised
/// after the injection goes to A, and the result names A as the turn's
/// approver." The first, held turn is started through the same client.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn steered_permission_after_injection_goes_to_the_turn_approver() {
    // Arrange: a session owned by neither approver, and a held turn that the
    // owner started naming approver A; the agent steers, and asks a
    // permission in the running turn right after an injection.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = ComposedHost::start(
        cursor_reactive(
            &receipt,
            &[
                "hold-first-prompt-open",
                "advertise-steering",
                "permission-after-steer",
            ],
        ),
        "cursor-local",
    )
    .await;
    let owner = ComposedHost::human("session-owner");
    let approver_a = ComposedHost::human("approver-a");
    let approver_b = ComposedHost::human("approver-b");
    let target = host.create(&owner).await;
    let first = OperationId::generate();
    let held = send(
        &host,
        &target,
        &owner,
        &approver_a,
        &first,
        "first",
        SHORT_WAIT,
    )
    .await;
    until_recorded(&receipt, "session/prompt", 1).await;

    // Act: B's message is steered; the permission that follows is decided
    // by whoever it reached; then the turn ends.
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
    let approval = pending_approval(&host, &receipt, "steer-permission-1").await;
    allow_once_as_its_approver(&host, &approval).await;
    let answer = until_answered(&receipt, "steer-permission-1").await;
    release(&receipt);
    let first_settled = settled(&host, &first).await;
    let approvals = host
        .client
        .list_approvals_with_options(false)
        .await
        .expect("approval history")
        .approvals;
    let steers = carrying(&receipt, "_session/steering", "second");
    let prompts = prompt_texts(&receipt);
    let cancels = recorded(&receipt, "session/cancel");
    let events = input_events(&host, &target).await;
    host.runtime.shutdown().await.expect("Host shutdown");
    let turn_inputs: Vec<&String> = events
        .iter()
        .filter(|(kind, _)| kind == "turnStarted")
        .map(|(_, input_id)| input_id)
        .collect();
    let injected = events
        .iter()
        .filter(|(kind, input_id)| kind == "inputAccepted" && !turn_inputs.contains(&input_id))
        .count();
    assert_eq!(
        (turn_inputs.len(), injected),
        (1, 1),
        "one turn ran, and the running turn took one more input under its own ID: {events:?}"
    );

    // Assert: steered under B's ID, naming A; the permission reached A only.
    assert!(
        matches!(held, Ok(ConversationOperationResult::Pending { .. })),
        "the first turn is held: {held:?}"
    );
    match steered {
        Ok(ConversationOperationResult::Completed {
            operation_id,
            settlement,
            ..
        }) => {
            assert_eq!(
                operation_id.as_ref(),
                Some(&second),
                "under the caller's ID"
            );
            match settlement.detail {
                ConversationSettlementDetail::ProviderSteered { turn_approver } => assert_eq!(
                    turn_approver,
                    Some(approver_a.clone()),
                    "the result names the running turn's approver"
                ),
                other => panic!("expected steered, got {other:?}"),
            }
        }
        other => panic!("expected a steered completion, got {other:?}"),
    }
    assert_eq!(
        approval.approver,
        approver_a.to_board_identity().expect("A"),
        "the permission after the injection goes to A: {approval:?}"
    );
    assert!(same_session(&approval, &target), "{approval:?}");
    assert_eq!(
        approvals.len(),
        1,
        "one permission, asked once: {approvals:?}"
    );
    assert_eq!(
        answer.pointer("/result/outcome/optionId"),
        Some(&Value::from("allow")),
        "the decision reached the agent: {answer}"
    );
    assert!(
        matches!(
            first_settled,
            Ok(ConversationOperationSettlement::PromptCompleted { .. })
        ),
        "the held turn ran to its end: {first_settled:?}"
    );
    assert_eq!(steers, 1, "the message was steered once");
    assert_eq!(prompts, vec!["first".to_owned()], "and never prompted");
    assert_eq!(cancels, 0, "no send interrupts a turn");
}

/// Oracle: D1 V22 (R12, C1) — "the held turn is owned by approver A; the
/// second message names approver B. ... Queued: its own turn's permission
/// request goes to B", and "the non-steering fixture records no second
/// prompt until the first turn ends, then exactly one".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_turn_permission_goes_to_its_own_approver() {
    // Arrange: a held turn owned by A on an agent that can't steer.
    let staging = tempfile::tempdir().expect("receipt folder");
    let receipt = staging.path().join("wire.json");
    let host = ComposedHost::start(
        cursor_reactive(&receipt, &["hold-first-prompt", "prompt-directives"]),
        "cursor-local",
    )
    .await;
    let owner = ComposedHost::human("session-owner");
    let approver_a = ComposedHost::human("approver-a");
    let approver_b = ComposedHost::human("approver-b");
    let target = host.create(&owner).await;
    let first = OperationId::generate();
    let held = send(
        &host,
        &target,
        &owner,
        &approver_a,
        &first,
        "first",
        SHORT_WAIT,
    )
    .await;
    until_recorded(&receipt, "session/prompt", 1).await;

    // Act: B's message waits in the line; the turn ends; B's own turn asks a
    // permission, decided by whoever it reached.
    let second = OperationId::generate();
    let queued = send_with_delivery(
        &host,
        &target,
        (&approver_b, &approver_b),
        &second,
        "ask-permission",
        SHORT_WAIT,
        collaboration_protocol::MessageDelivery::Queue,
    )
    .await;
    let prompts_while_held = prompt_texts(&receipt);
    let queued_input = show(&host, &second)
        .await
        .and_then(|snapshot| snapshot.input_id)
        .map(String::from);
    let pending_while_held = host
        .client
        .list_approvals_with_options(true)
        .await
        .expect("pending approvals")
        .approvals;
    release(&receipt);
    let approval = pending_approval(&host, &receipt, "permission-1").await;
    allow_once_as_its_approver(&host, &approval).await;
    let answer = until_answered(&receipt, "permission-1").await;
    let second_settled = settled(&host, &second).await;
    let prompts_after = prompt_texts(&receipt);
    let cancels = recorded(&receipt, "session/cancel");
    let events = input_events(&host, &target).await;
    host.runtime.shutdown().await.expect("Host shutdown");
    let queued_input = queued_input.expect("show names the waiting operation's input ID");
    assert!(
        events
            .iter()
            .any(|(kind, input_id)| kind == "turnStarted" && input_id == &queued_input),
        "its own turn started under the operation's input ID {queued_input}: {events:?}"
    );

    // Assert: queued under B's ID, run once after the turn, approved by B.
    assert!(
        matches!(held, Ok(ConversationOperationResult::Pending { .. })),
        "the first turn is held: {held:?}"
    );
    assert!(
        matches!(&queued, Ok(ConversationOperationResult::Queued { operation_id, .. }) if operation_id == &second),
        "the second message is queued under the caller's ID: {queued:?}"
    );
    assert_eq!(
        prompts_while_held,
        vec!["first".to_owned()],
        "no second prompt while the first turn runs"
    );
    assert!(pending_while_held.is_empty(), "{pending_while_held:?}");
    assert_eq!(
        approval.approver,
        approver_b.to_board_identity().expect("B"),
        "the queued turn's permission goes to B: {approval:?}"
    );
    assert!(same_session(&approval, &target), "{approval:?}");
    assert_eq!(
        answer.pointer("/result/outcome/optionId"),
        Some(&Value::from("allow")),
        "the decision reached the agent: {answer}"
    );
    assert!(
        matches!(
            second_settled,
            Ok(ConversationOperationSettlement::PromptCompleted { .. })
        ),
        "the queued prompt ran to its end: {second_settled:?}"
    );
    assert_eq!(
        prompts_after,
        vec!["first".to_owned(), "ask-permission".to_owned()],
        "then exactly one more prompt"
    );
    assert_eq!(cancels, 0, "no send interrupts a turn");
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
            "provider binding has unresolved operations: {}",
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
/// ends, the queued entry ends "not sent: an earlier outcome is unknown",
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
        "not sent: an earlier outcome is unknown",
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
    let mailed = host
        .client
        .send_message(collaboration_client::MessageSendRequest {
            item_id: None,
            target: target.clone(),
            message: collaboration_client::PublicMessageContent::HumanUser {
                text: collaboration_protocol::MessageText::try_from("first".to_owned())
                    .expect("mail text"),
            },
            delivery: collaboration_protocol::MessageDelivery::Auto,
            generation_guard: None,
        })
        .await
        .expect("mail receipt");
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
            &mailed,
            collaboration_protocol::MessageSendResult::Local(sent)
                if matches!(sent.receipt.outcome, collaboration_protocol::DeliveryOutcome::Started)
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
