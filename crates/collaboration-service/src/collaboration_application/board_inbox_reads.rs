//! Inbox reads retain watched scope boundaries and project-local acknowledgements.
use super::board_paging::fingerprint;
use super::board_record_paging::FeedPageSource;
use super::{BoardOperations, admission_error};
use message_board::*;
#[derive(serde::Serialize)]
struct InboxReadFilter<'query> {
    scope: &'query InboxScope,
    reader: &'query Identity,
    read_mode: InboxReadMode,
}
impl BoardOperations<'_> {
    pub async fn inbox_fetch(
        &self,
        request: InboxFetchRequest,
    ) -> Result<InboxFetchResult, BoardError> {
        let filter = fingerprint(&InboxReadFilter {
            scope: &request.scope,
            reader: &request.reader,
            read_mode: request.read_mode,
        })?;
        // Validate the read before any independently requested initialization can write.
        let prior = self
            .decode_read_cursor(&request.page, "inbox_fetch", &filter)
            .await?;
        let mut initializations = Vec::new();
        for entry in request.initializations() {
            let record = self.fleet_record(entry.project_id.clone().into()).await?;
            let allowed = match &request.scope {
                InboxScope::Project { project_id } => project_id == &entry.project_id,
                InboxScope::Workspace { workspace_id } => {
                    workspace_id == &record.envelope().scope.workspace
                }
                InboxScope::Board { board_id } => {
                    record.envelope().scope.board.as_ref() == Some(board_id)
                }
            };
            if !allowed {
                return Err(BoardError::invalid_field(
                    "initializations",
                    "must belong to the inbox scope",
                ));
            }
        }
        for entry in request.initializations() {
            let result = self
                .project_write(ProjectRequest {
                    request_id: entry.request_id.clone(),
                    project_id: entry.project_id.clone(),
                    actor: request.reader.clone(),
                    acting_for: request.acting_for.clone(),
                    observed_epoch: entry.observed_epoch,
                    resubmits: entry.resubmits.clone(),
                    protocol_version: request.protocol_version,
                    operation: BoardOperation::ReadMarkInitialize {
                        reader: request.reader.clone(),
                        scope: ReadScope::Project {
                            project_id: entry.project_id.clone(),
                        },
                    },
                })
                .await;
            initializations.push(ReadInitializationOutcome {
                project_id: entry.project_id.clone(),
                request_id: entry.request_id.clone(),
                outcome: match result {
                    Ok(reply) => ProjectWriteCallResult::Replied { reply },
                    Err(reason) => ProjectWriteCallResult::Unanswered { reason },
                },
            });
        }
        let scope = match &request.scope {
            InboxScope::Workspace { workspace_id } => PostListScope::Workspace {
                workspace_id: workspace_id.clone(),
            },
            InboxScope::Board { board_id } => PostListScope::Board {
                board_id: board_id.clone(),
            },
            InboxScope::Project { project_id } => PostListScope::Project {
                project_id: project_id.clone(),
            },
        };
        let workspace_id = match &request.scope {
            InboxScope::Workspace { workspace_id } => workspace_id.clone(),
            InboxScope::Board { board_id } => {
                self.board_show(BoardShowRequest {
                    board_id: board_id.clone(),
                })
                .await?
                .board
                .workspace_id
            }
            InboxScope::Project { project_id } => self
                .fleet_record(project_id.clone().into())
                .await?
                .envelope()
                .scope
                .workspace
                .clone(),
        };
        let selection = if request.read_mode == InboxReadMode::Latest {
            PostSelection::Latest
        } else {
            PostSelection::AfterPosition {
                after: ReadPosition::Aggregate {
                    cursor: AggregateCursor::default(),
                },
            }
        };
        let post_request = PostListRequest {
            scope,
            selection,
            page: request.page,
        };
        let page = self
            .project_feed_page(
                &post_request,
                ("inbox_fetch", filter, prior),
                FeedPageSource::Inbox {
                    reader: &request.reader,
                    mode: request.read_mode,
                },
                super::board_inbox_events::inbox_activity,
            )
            .await?;
        Ok(InboxFetchResult {
            page: InboxPage {
                workspace_id,
                scope: request.scope,
                reader: request.reader,
                read_mode: request.read_mode,
                ordering: page.ordering,
                high_water: page.high_water,
                next: page.next,
                unavailable: page.unavailable,
                records: page.records,
                next_cursor: page.next_cursor,
                initializations,
            },
        })
    }

    pub async fn inbox_workspaces(
        &self,
        request: InboxWorkspacesRequest,
    ) -> Result<InboxWorkspacesResult, BoardError> {
        let filter = fingerprint(&request)?;
        let mut after = self
            .metadata_after(&request.page, "inbox_workspaces", &filter)
            .await?;
        let limit = usize::try_from(request.page.limit.get())
            .map_err(|_| BoardError::board_unavailable())?;
        let mut summaries = Vec::new();
        loop {
            let keys = self
                .admission()?
                .fleet
                .workspace_page_keys(&after, u64::from(request.page.limit.get()) + 1)
                .await
                .map_err(|error| admission_error(error.into()))?;
            let exhausted = keys.len() <= limit;
            for workspace in keys {
                after = workspace.as_str().to_owned();
                let Some(summary) = self
                    .workspace_unread_summary(&workspace, &request.reader)
                    .await?
                else {
                    continue;
                };
                if !request.unread_only || summary.has_unread {
                    summaries.push(summary);
                }
                if summaries.len() > limit {
                    break;
                }
            }
            if summaries.len() > limit || exhausted {
                break;
            }
        }
        let has_more = summaries.len() > limit;
        summaries.truncate(limit);
        Ok(InboxWorkspacesResult {
            page: self
                .selected_metadata_page(
                    summaries,
                    request.page,
                    "inbox_workspaces",
                    filter,
                    has_more,
                    |summary| summary.workspace_id.as_str().to_owned(),
                )
                .await?,
        })
    }
    async fn workspace_unread_summary(
        &self,
        workspace: &WorkspaceId,
        reader: &Identity,
    ) -> Result<Option<WorkspaceUnreadSummary>, BoardError> {
        let admission = self.admission()?;
        let projects = admission
            .fleet
            .workspace_projects(workspace, &admission.registry)
            .await
            .map_err(|error| admission_error(error.into()))?;
        let mut interested = false;
        let mut initialized = !projects.is_empty();
        let mut unread = false;
        // A summary answers whether THIS workspace has activity. Foreign watched context
        // remains in fetch, but cannot create or conceal this workspace's unread flag.
        for project in projects {
            let Some(store) = admission
                .registry
                .existing_project(&project)
                .await
                .map_err(|error| admission_error(error.into()))?
            else {
                initialized = false;
                continue;
            };
            let mut read = store
                .snapshot()
                .await
                .map_err(|error| admission_error(error.into()))?;
            let tracking = read
                .reader_tracking_metadata(reader)
                .await
                .map_err(|error| admission_error(error.into()))?;
            interested |= tracking.has_interest;
            initialized &= tracking.main_initialized;
            if tracking.has_interest && !unread {
                let bounds = message_board_storage::FeedPageBounds {
                    after: read
                        .inbox_scan_start(reader, true)
                        .await
                        .map_err(|error| admission_error(error.into()))?
                        .unwrap_or(read.high_water().position),
                    through: read.high_water().position,
                    direction: message_board_storage::FeedPageDirection::Forward,
                    limit: 1,
                    sweep: None,
                };
                unread = !read
                    .inbox_page_keys(&bounds, reader, InboxReadMode::Unread, true)
                    .await
                    .map_err(|error| admission_error(error.into()))?
                    .is_empty();
            }
            read.finish()
                .await
                .map_err(|error| admission_error(error.into()))?;
        }
        Ok(interested.then(|| WorkspaceUnreadSummary {
            workspace_id: workspace.clone(),
            has_unread: unread,
            main_tracking_initialized: initialized,
        }))
    }
}
