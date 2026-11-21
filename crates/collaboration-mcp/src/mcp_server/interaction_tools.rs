//! Provider settings and Session lifecycle tools.
use super::{
    CallToolResult, CollaborationMcpServer, ConversationCloseRequest,
    ConversationOperationSubmission, ConversationResumeRequest, McpToolOutput, OperationEffect,
    Parameters, ProviderSettingsAcceptRequest, ProviderSettingsFailure, ProviderSettingsResult,
    ProviderSettingsSetRequest, tool, tool_router, typed_failure_result,
};

#[tool_router(router = provider_control_tool_router, vis = "pub(super)")]
impl CollaborationMcpServer {
    #[tool(name = "conversation_settings_set", description = "Sets one advertised provider Session mode, model, or effort, or fast (off or auto), as its creator or Approver. An ambiguous provider response leaves settings gated.", output_schema = rmcp::handler::server::tool::schema_for_type::<McpToolOutput<ProviderSettingsResult>>())]
    pub(super) async fn conversation_settings_set(
        &self,
        Parameters(request): Parameters<ProviderSettingsSetRequest>,
    ) -> CallToolResult {
        typed_failure_result::<ProviderSettingsFailure, _, _>(
            self.application.conversations().settings_set(request).await,
            OperationEffect::Unknown,
        )
    }

    #[tool(name = "conversation_settings_accept", description = "Accepts the provider Session's currently reported settings as its creator or Approver and clears the prompt gate without an agent RPC.", output_schema = rmcp::handler::server::tool::schema_for_type::<McpToolOutput<ProviderSettingsResult>>())]
    pub(super) async fn conversation_settings_accept(
        &self,
        Parameters(request): Parameters<ProviderSettingsAcceptRequest>,
    ) -> CallToolResult {
        typed_failure_result::<ProviderSettingsFailure, _, _>(
            self.application
                .conversations()
                .settings_accept(request)
                .await,
            OperationEffect::None,
        )
    }

    #[tool(name = "conversation_resume", description = "Resumes one advertised provider Session as an inspectable operation. The agent supplies no history replay; inspect the operation ID after an uncertain response.", output_schema = rmcp::handler::server::tool::schema_for_type::<McpToolOutput<ConversationOperationSubmission>>())]
    pub(super) async fn conversation_resume(
        &self,
        Parameters(request): Parameters<ConversationResumeRequest>,
    ) -> CallToolResult {
        typed_failure_result::<collaboration_protocol::ConversationOperationFailure, _, _>(
            self.application
                .conversations()
                .conversation_resume(request)
                .await,
            OperationEffect::Unknown,
        )
    }

    #[tool(name = "conversation_close", description = "Closes one provider Session after its running Turn settles. The operation ID remains inspectable after an uncertain response.", output_schema = rmcp::handler::server::tool::schema_for_type::<McpToolOutput<ConversationOperationSubmission>>())]
    pub(super) async fn conversation_close(
        &self,
        Parameters(request): Parameters<ConversationCloseRequest>,
    ) -> CallToolResult {
        typed_failure_result::<collaboration_protocol::ConversationOperationFailure, _, _>(
            self.application
                .conversations()
                .conversation_close(request)
                .await,
            OperationEffect::Unknown,
        )
    }
}
