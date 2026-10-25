//! Typed read board calls; uncertain writes are never retransmitted.
use super::{BoardClientError, CollaborationClient};
use crate::ClientError;
use message_board::public_replies::{RequestEffectsRead, RequestEffectsRequest};
use message_board::*;
impl CollaborationClient {
    pub async fn request_effects(
        &self,
        request: RequestEffectsRequest,
    ) -> Result<RequestEffectsRead, BoardClientError> {
        self.board_call("request_effects", request).await
    }

    pub async fn discovery_search(
        &self,
        request: DiscoverySearchRequest,
    ) -> Result<DiscoverySearchResult, BoardClientError> {
        self.board_call("discovery_search", request).await
    }
    pub async fn post_search(
        &self,
        request: PostSearchRequest,
    ) -> Result<PostSearchResult, BoardClientError> {
        self.board_call("post_search", request).await
    }

    pub async fn post_show(
        &self,
        request: PostShowRequest,
    ) -> Result<PostShowResult, BoardClientError> {
        self.board_call("post_show", request).await
    }
    pub async fn post_list(
        &self,
        request: PostListRequest,
    ) -> Result<PostListResult, BoardClientError> {
        self.board_call("post_list", request).await
    }
    pub async fn inbox_fetch(
        &self,
        request: InboxFetchRequest,
    ) -> Result<InboxFetchResult, BoardClientError> {
        if request.initializations().is_empty() {
            self.board_call("inbox_fetch", request).await
        } else {
            let resource = match &request.scope {
                InboxScope::Workspace { workspace_id } => ResourceIdentity::Workspace {
                    workspace_id: workspace_id.clone(),
                },
                InboxScope::Board { board_id } => ResourceIdentity::Board {
                    board_id: board_id.clone(),
                },
                InboxScope::Project { project_id } => ResourceIdentity::Project {
                    project_id: project_id.clone(),
                },
            };
            self.board_mutation_call("inbox_fetch", request, resource)
                .await
        }
    }
    pub async fn inbox_acknowledge(
        &self,
        request: InboxAcknowledgeRequest,
    ) -> Result<InboxAcknowledgeReply, BoardClientError> {
        let entry = request
            .entries()
            .first()
            .ok_or(ClientError::InvalidRequest("empty acknowledgement"))?;
        let resource = match &entry.scope {
            ReadScopeRef::Project { project_id } => ResourceIdentity::Project {
                project_id: project_id.clone(),
            },
            ReadScopeRef::Thread { thread } => ResourceIdentity::Thread {
                thread_id: thread.thread.clone(),
            },
        };
        self.board_mutation_call("inbox_acknowledge", request, resource)
            .await
    }
    pub async fn inbox_workspaces(
        &self,
        request: InboxWorkspacesRequest,
    ) -> Result<InboxWorkspacesResult, BoardClientError> {
        self.board_call("inbox_workspaces", request).await
    }
}
