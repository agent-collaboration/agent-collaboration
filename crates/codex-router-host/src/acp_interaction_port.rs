//! Stateless policy for ACP callbacks in the managed provider runtime.

use acp_client_runtime::{
    ApprovalPortOutcome, InteractionFuture, InteractionPort, RefusedApprovalOffer, SessionEventSink,
};
use collaboration_protocol::OperationId;
use session_event_model::{
    ApprovalRequest, QuestionRequest, QuestionResponse,
    managed_interaction_refusal::ManagedInteractionRefusal,
};
use tokio_util::sync::CancellationToken;

use crate::ExternalProviderApprovalContext;

pub(crate) struct HostInteractionPort;

impl InteractionPort for HostInteractionPort {
    type Context = ExternalProviderApprovalContext;
    type OperationId = OperationId;

    fn managed_refusal(&self) -> Option<ManagedInteractionRefusal> {
        Some(ManagedInteractionRefusal::ApprovalsUnavailableOnBuild)
    }

    fn operation_id(context: &Self::Context) -> Self::OperationId {
        context.operation_id.clone()
    }

    fn binding_retirement(context: &Self::Context) -> CancellationToken {
        context.binding_retirement.clone()
    }

    fn request_approval(
        &self,
        _context: Self::Context,
        _request: ApprovalRequest,
        _turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        Box::pin(async { ApprovalPortOutcome::Unavailable })
    }

    fn request_question(
        &self,
        _context: Self::Context,
        _request: QuestionRequest,
        _turn_cancellation: CancellationToken,
        _agent_cancellation: CancellationToken,
    ) -> InteractionFuture<'_, QuestionResponse> {
        Box::pin(async { QuestionResponse::Cancelled })
    }

    fn record_refusal(
        &self,
        _context: Self::Context,
        _refusal: RefusedApprovalOffer,
    ) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }

    fn cancel_all(
        &self,
        _context: Self::Context,
        _reason: &'static str,
    ) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }

    fn cancel_retired(&self) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// Test-only stand-in for direct runtime fixtures without a Router Host.
pub(crate) struct NoopSessionEventSink;

impl SessionEventSink for NoopSessionEventSink {
    fn publish_skill_view(
        &self,
        _session_id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), acp_client_runtime::EventSinkClosed> {
        Ok(())
    }

    fn begin_history_replay(
        &self,
        _session_id: &str,
    ) -> acp_client_runtime::HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    fn publish(
        &self,
        _session_id: &str,
        _event: session_event_model::SessionEvent,
    ) -> Result<(), acp_client_runtime::EventSinkClosed> {
        Ok(())
    }
}
