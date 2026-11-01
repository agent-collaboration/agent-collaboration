//! Preparation uses the resolved call binding, before the carrier can create anything.
use super::session_binding::CallSessionAdmission;
use super::{CallToolResult, CollaborationMcpServer, structured_tool_error};
use collaboration_contracts::HarnessLaunchFailure;
use collaboration_protocol::{ConversationOperationFailureStage, ProviderOperationEffect};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum PreparationRefusalKind {
    StartPreparationUnavailable,
    StartPreparationInvalidInput,
    ProjectRequired,
    StartRefused,
    InactiveSessionBinding,
    OperationConflict,
}

#[derive(serde::Serialize)]
struct PreparationRefusal<TReason> {
    kind: PreparationRefusalKind,
    stage: ConversationOperationFailureStage,
    effect: ProviderOperationEffect,
    message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<TReason>,
}
fn refusal(failure: HarnessLaunchFailure) -> CallToolResult {
    let (kind, message, reason) = match failure {
        HarnessLaunchFailure::Unavailable => (
            PreparationRefusalKind::StartPreparationUnavailable,
            "Session start preparation is unavailable",
            None,
        ),
        HarnessLaunchFailure::InvalidInput => (
            PreparationRefusalKind::StartPreparationInvalidInput,
            "Declared session launch input is invalid",
            None,
        ),
        HarnessLaunchFailure::ProjectRequired => (
            PreparationRefusalKind::ProjectRequired,
            "A session start must name its project",
            None,
        ),
        HarnessLaunchFailure::StartRefused(reason) => (
            PreparationRefusalKind::StartRefused,
            "The organization authority refused the session start",
            Some(reason),
        ),
        HarnessLaunchFailure::InactiveBinding => (
            PreparationRefusalKind::InactiveSessionBinding,
            "The calling session binding is inactive or unavailable",
            None,
        ),
        HarnessLaunchFailure::AlreadyPrepared | HarnessLaunchFailure::InvalidTransition => (
            PreparationRefusalKind::OperationConflict,
            "The retained operation cannot prepare this launch",
            None,
        ),
    };
    structured_tool_error(serde_json::json!(PreparationRefusal {
        kind,
        stage: ConversationOperationFailureStage::Admission,
        effect: ProviderOperationEffect::None,
        message,
        reason,
    }))
}
impl CollaborationMcpServer {
    pub(super) async fn prepare_bound_conversation(
        &self,
        declared: &collaboration_client::ConversationCreateInput,
    ) -> Option<CallToolResult> {
        let CallSessionAdmission::BoundHttp(caller) = &self.call_session else {
            return None;
        };
        let Some(port) = &self.bound_conversation_preparation else {
            return Some(refusal(HarnessLaunchFailure::Unavailable));
        };
        port.prepare(caller, declared).await.err().map(refusal)
    }
}
