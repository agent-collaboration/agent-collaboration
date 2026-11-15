//! The client waits the requested server window plus a transport allowance, no longer.
//!
//! Both the client's deadline and the test's advances use one injected `ManualClock`, so
//! the deadline is checked at exactly the times the test sets.

use crate::scripted_api::{SCRIPTED_SERVICE_EPOCH, SCRIPTED_SERVICE_ID};
use crate::{ClientError, CollaborationClient, scripted_api::ScriptedApi};
use collaboration_protocol::{
    ConversationOperationSnapshot, ConversationOperationWaitOutput,
    ConversationOperationWaitRequest, ConversationOperationWaitResult, OperationId,
    PositiveSeconds,
};
use router_clock::{ClockInstant, ManualClock};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

/// The window the client asks the Router to wait, plus the client's transport allowance.
const WAIT_DEADLINE: Duration = Duration::from_secs(65);

/// A pending wait result for `operation_id`, as the Router answers an unsettled operation.
fn pending_wait_result(operation_id: &Value) -> Value {
    let operation: ConversationOperationSnapshot = serde_json::from_value(json!({
        "operationId":operation_id,
        "operation":"conversationPrompt",
        "binding":{"kind":"externalProvider","binding":{
            "endpoint":{"serviceId":SCRIPTED_SERVICE_ID,"endpointId":"claude-local"},
            "bindingId":"binding",
            "runtime":{"provider":"claudeCode","runtimeName":"fixture","runtimeVersion":null},
            "transport":"stdioAcp",
            "generation":{"serviceEpoch":SCRIPTED_SERVICE_EPOCH,"generation":1},
            "capabilities":[{"name":"prompt","status":"supported","evidence":"advertised"}]
        }},
        "target":null,"stage":"mayHaveDispatched","effect":"unknown",
        "reconciliation":"unresolved","admittedAt":"2026-09-22T00:00:00.000Z","terminalAt":null
    }))
    .unwrap();
    serde_json::to_value(ConversationOperationWaitResult {
        operation,
        output: ConversationOperationWaitOutput::Pending,
    })
    .unwrap()
}

async fn connect_on_manual_clock(
    name: &str,
) -> (
    Arc<ManualClock>,
    ScriptedApi,
    crate::scripted_api::ScriptedCalls,
    CollaborationClient,
) {
    let clock = Arc::new(ManualClock::new("2026-11-15T00:00:00Z".parse().unwrap()));
    let (api, calls) = ScriptedApi::start_with_clock(clock.clone()).unwrap();
    let client = CollaborationClient::connect_with_clock(api.directory(), name, "1", clock.clone())
        .await
        .unwrap();
    (clock, api, calls, client)
}

fn sixty_second_wait(operation_id: OperationId) -> ConversationOperationWaitRequest {
    ConversationOperationWaitRequest {
        operation_id,
        timeout_seconds: PositiveSeconds::try_from(60).unwrap(),
    }
}

#[tokio::test]
async fn provider_wait_uses_requested_server_window_plus_transport_allowance() {
    let (clock, _api, mut calls, client) = connect_on_manual_clock("wait-proof").await;
    let operation_id = OperationId::generate();
    let request = sixty_second_wait(operation_id.clone());
    let mut waiting = tokio::spawn(async move {
        client
            .wait_for_provider_conversation_operation(request)
            .await
    });
    let call = calls.next().await.unwrap();
    assert_eq!(call.tool, "conversation_operation_wait");
    assert_eq!(call.arguments["timeoutSeconds"], 60);
    // The Router settles the wait 40 seconds after receiving it: the client's deadline
    // must have been checked at 40 seconds and found not due.
    clock.advance(Duration::from_secs(40)).unwrap();
    tokio::select! {
        result = &mut waiting => panic!("wait ended inside its window: {result:?}"),
        () = clock.not_due_check_now(ClockInstant::ZERO.after(WAIT_DEADLINE)) => {}
    }
    let result = pending_wait_result(&call.arguments["operationId"]);
    call.succeed(result);
    let result = waiting
        .await
        .unwrap()
        .expect("40-second settlement remains within requested wait window");
    assert_eq!(result.operation.operation_id, operation_id);
    assert!(matches!(
        result.output,
        ConversationOperationWaitOutput::Pending
    ));
}

#[tokio::test]
async fn provider_wait_reports_true_transport_timeout_after_allowance() {
    let (clock, _api, mut calls, client) = connect_on_manual_clock("timeout-proof").await;
    let request = sixty_second_wait(OperationId::generate());
    let mut waiting = tokio::spawn(async move {
        client
            .wait_for_provider_conversation_operation(request)
            .await
    });
    // The Router never answers: the call stays open past the 60-second window and its
    // 5-second transport allowance.
    let unanswered = calls.next().await.unwrap();
    clock.advance(Duration::from_secs(64)).unwrap();
    tokio::select! {
        result = &mut waiting => panic!("wait ended before window plus allowance: {result:?}"),
        () = clock.not_due_check_now(ClockInstant::ZERO.after(WAIT_DEADLINE)) => {}
    }
    clock.advance(Duration::from_secs(1)).unwrap();
    assert!(matches!(waiting.await.unwrap(), Err(ClientError::Timeout)));
    drop(unanswered);
}
