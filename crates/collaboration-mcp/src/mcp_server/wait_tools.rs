//! Bounded waits: a reader's subscription batch and a wake-up's first fire.
//!
//! Each wait is one call bounded by its own limit. A caller that goes away cancels the call,
//! which drops the wait and releases what it holds.
use super::board_argument_classification::UndecodedBoardArguments;
use super::{
    API_RESULT_BUDGET, CallToolResult, CollaborationMcpServer, McpToolOutput, OperationEffect,
    Parameters, ThreadSubscriptionWaitRequest, ThreadSubscriptionWaitResult, domain_result,
    operation_failure_result, rejection_failure, success_result, thread_wait_outcome_unknown, tool,
    tool_router, validation_failure, wake_wait_failure,
};
use collaboration_protocol::WakeWaitRequest;
use collaboration_service::collaboration_application::{
    WakeWaitFailure, thread_wait_root_notice_limit,
};
use collaboration_service::{WakeOperationFailure, WakeWaitResponse};

#[tool_router(router = wait_tool_router, vis = "pub(super)")]
impl CollaborationMcpServer {
    #[tool(
        name = "thread_wait",
        description = "Waits for one due batch from the supplied reader's poll-mode subscriptions, filtered to all scopes, selected roots or one project. Returns an empty batch at maxWaitSeconds when nothing is due. A handed batch remains unread until the reader acknowledges it.",
        output_schema = rmcp::handler::server::tool::schema_for_type::<
            McpToolOutput<ThreadSubscriptionWaitResult>,
        >()
    )]
    pub(super) async fn thread_wait(
        &self,
        Parameters(arguments): Parameters<UndecodedBoardArguments<ThreadSubscriptionWaitRequest>>,
        context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> CallToolResult {
        let request = match arguments.decode("thread_wait") {
            Ok(request) => request,
            Err(rejection) => return domain_result::<(), _>(Err(rejection), true),
        };
        let Ok(maximum_root_notice_bytes) = thread_wait_root_notice_limit(API_RESULT_BUDGET) else {
            return validation_failure(
                "The tool result budget leaves no room for one subscription root.",
            );
        };
        let (actor, filter, renewals) = (
            request.actor.clone(),
            request.filter.clone(),
            request.renewals.clone(),
        );
        let board = self.application.board();
        tokio::select! {
            () = context.ct.cancelled() => thread_wait_outcome_unknown(&actor, &filter, &renewals),
            result = board.thread_wait(request, maximum_root_notice_bytes) => {
                domain_result(result, true)
            }
        }
    }

    #[tool(name = "wake_wait_until_first_fire", description = "Waits up to timeoutSeconds (1 to 1500, default 60) for the Wake's first timer fire. Unmoved local Wakes retain their cursor wait; central Wakes use only their Wake ID and return the actual first Firing and its state, a lifecycle outcome, a distinct faulted envelope, or pending on timeout/outage. Central waits sleep to the known grid due time before bounded read-only backoff and never cause a Firing. Delivery is inspected separately. Cancellation ends this wait without recreating or replaying the Wake.", output_schema = rmcp::handler::server::tool::schema_for_type::<McpToolOutput<WakeWaitResponse>>())]
    pub(super) async fn wake_wait_until_first_fire(
        &self,
        Parameters(request): Parameters<WakeWaitRequest>,
        context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> CallToolResult {
        let wakeup_id = request.wakeup_id.clone();
        let wakes = self.application.wakes();
        let result = tokio::select! {
            () = context.ct.cancelled() => {
                return wake_wait_failure(collaboration_client::WakeWaitError::CallerCancelled {
                    wakeup_id,
                });
            }
            result = wakes.wake_wait_until_first_fire(request) => result,
        };
        match result {
            Ok(outcome) => success_result(&outcome),
            Err(WakeOperationFailure::Wait(failure))
                if matches!(*failure, WakeWaitFailure::NotFound(_)) =>
            {
                wake_wait_failure(collaboration_client::WakeWaitError::NotFound { wakeup_id })
            }
            Err(WakeOperationFailure::Wait(failure))
                if matches!(*failure, WakeWaitFailure::Unavailable(_)) =>
            {
                wake_wait_failure(collaboration_client::WakeWaitError::Unavailable { wakeup_id })
            }
            Err(rejection) => {
                operation_failure_result(&rejection_failure(&rejection, OperationEffect::None))
            }
        }
    }
}
