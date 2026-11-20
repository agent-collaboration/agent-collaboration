//! Bounded waits: a reader's subscription batch.
//!
//! Each wait is one call bounded by its own limit. A caller that goes away cancels the call,
//! which drops the wait and releases what it holds.
use super::board_argument_classification::UndecodedBoardArguments;
use super::{
    API_RESULT_BUDGET, CallToolResult, CollaborationMcpServer, McpToolOutput, Parameters,
    ThreadSubscriptionWaitRequest, ThreadSubscriptionWaitResult, domain_result,
    thread_wait_outcome_unknown, tool, tool_router, validation_failure,
};
use collaboration_service::collaboration_application::thread_wait_root_notice_limit;

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
}
