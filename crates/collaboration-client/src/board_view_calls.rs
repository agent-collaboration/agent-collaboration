//! Views and hooks use the same typed tools as every other board surface.
use super::{BoardClientError, CollaborationClient};
use message_board::*;
impl CollaborationClient {
    pub async fn view_work(
        &self,
        request: ProjectViewRequest,
    ) -> Result<WorkView, BoardClientError> {
        self.board_call("view_work", request).await
    }
    pub async fn view_card(
        &self,
        request: ContinuationCardRequest,
    ) -> Result<ContinuationCard, BoardClientError> {
        self.board_call("view_card", request).await
    }
    pub async fn view_owner(
        &self,
        request: OwnerViewRequest,
    ) -> Result<OwnerView, BoardClientError> {
        self.board_call("view_owner", request).await
    }
    pub async fn view_lead_load(
        &self,
        request: LeadLoadRequest,
    ) -> Result<LeadLoad, BoardClientError> {
        self.board_call("view_lead_load", request).await
    }
    pub async fn view_session_context(
        &self,
        request: SessionContextRequest,
    ) -> Result<HookPayload, BoardClientError> {
        self.board_call("view_session_context", request).await
    }
    pub async fn hook_context(
        &self,
        request: SessionContextRequest,
    ) -> Result<HookResult, BoardClientError> {
        self.board_call("hook_context", request).await
    }
    pub async fn hook_check(
        &self,
        request: ProjectMoveCheckRequest,
    ) -> Result<HookResult, BoardClientError> {
        self.board_call("hook_check", request).await
    }
    pub async fn hook_card(
        &self,
        request: ContinuationCardRequest,
    ) -> Result<HookResult, BoardClientError> {
        self.board_call("hook_card", request).await
    }
}
