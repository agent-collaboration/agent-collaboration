//! Post reads select scoped project keys before loading any page bodies.
use super::board_paging::fingerprint;
use super::board_record_paging::FeedPageSource;
use super::{BoardOperations, admission_error};
use message_board::{
    BoardError, FeedContent, FeedEntry, LocatedPost, Page, Post, PostListRequest, PostListResult,
    PostListScope, PostPage, PostSearchRequest, PostSearchResult, PostSelection, ProjectId, Record,
    SearchPostKind,
};

impl BoardOperations<'_> {
    pub(super) async fn projects_for_post_scope(
        &self,
        scope: &PostListScope,
    ) -> Result<Vec<ProjectId>, BoardError> {
        let fleet = &self.admission()?.fleet;
        match scope {
            PostListScope::Project { project_id } => Ok(vec![project_id.clone()]),
            PostListScope::Thread { thread } => Ok(vec![self.thread_project(thread).await?]),
            PostListScope::Workspace { workspace_id } => fleet
                .workspace_projects(workspace_id, &self.admission()?.registry)
                .await
                .map_err(|error| admission_error(error.into())),
            PostListScope::Board { board_id } => fleet
                .scoped_projects(Some(board_id), &self.admission()?.registry)
                .await
                .map_err(|error| admission_error(error.into())),
            PostListScope::AllWorkspaces => fleet
                .scoped_projects(None, &self.admission()?.registry)
                .await
                .map_err(|error| admission_error(error.into())),
        }
    }
    pub async fn post_list(&self, request: PostListRequest) -> Result<PostListResult, BoardError> {
        let filter = fingerprint(&request)?;
        self.read_post_page(request, "post_list", filter).await
    }
    pub(super) async fn read_post_page(
        &self,
        request: PostListRequest,
        operation: &str,
        filter: String,
    ) -> Result<PostListResult, BoardError> {
        let prior = self
            .decode_read_cursor(&request.page, operation, &filter)
            .await?;
        let page = self
            .project_feed_page(
                &request,
                (operation, filter, prior),
                FeedPageSource::Posts {
                    search: None,
                    active_only: false,
                },
                shown_post,
            )
            .await?;
        Ok(PostListResult {
            page: PostPage {
                scope: request.scope,
                selection: request.selection,
                ordering: page.ordering,
                records: page.records,
                next: page.next,
                high_water: page.high_water,
                unavailable: page.unavailable,
                next_cursor: page.next_cursor,
            },
        })
    }
    pub async fn post_search(
        &self,
        request: PostSearchRequest,
    ) -> Result<PostSearchResult, BoardError> {
        let filter = fingerprint(&request)?;
        if matches!(request.scope, PostListScope::Thread { .. })
            && request.kind == SearchPostKind::TopLevel
        {
            return Err(BoardError::invalid_field(
                "kind",
                "thread scope excludes top-level search",
            ));
        }
        let prior = self
            .decode_read_cursor(&request.page, "post_search", &filter)
            .await?;
        let query = request.query.as_str().to_ascii_lowercase();
        let list = PostListRequest {
            scope: request.scope,
            selection: PostSelection::Latest,
            page: request.page,
        };
        let page = self
            .project_feed_page(
                &list,
                ("post_search", filter, prior),
                FeedPageSource::Posts {
                    search: Some((&query, request.kind)),
                    active_only: !request.include_archived,
                },
                |entry| {
                    let post = shown_post(entry)?;
                    Ok(LocatedPost {
                        workspace_id: post.envelope.scope.workspace.clone(),
                        post,
                    })
                },
            )
            .await?;
        if !page.unavailable.is_empty() {
            return Err(BoardError::board_unavailable());
        }
        Ok(PostSearchResult {
            page: Page {
                records: page.records,
                next_cursor: page.next_cursor,
            },
        })
    }
}
fn shown_post(entry: FeedEntry) -> Result<Post, BoardError> {
    if let FeedContent::Shown(record) = entry.content
        && let Record::Post(post) = *record
    {
        Ok(post)
    } else {
        Err(BoardError::board_unavailable())
    }
}
