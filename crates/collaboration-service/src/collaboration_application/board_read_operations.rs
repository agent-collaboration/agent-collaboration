//! Fleet discovery and project snapshots stay outside the writer transaction.
use super::board_paging::fingerprint;
use super::{BoardOperations, admission_error};
use message_board::*;

impl BoardOperations<'_> {
    pub async fn project_show(
        &self,
        request: ProjectShowRequest,
    ) -> Result<ProjectShowResult, BoardError> {
        match self.fleet_record(request.project_id.clone().into()).await? {
            Record::DirectoryEntry(project) => {
                let state = match self
                    .admission()?
                    .registry
                    .read_project(&request.project_id)
                    .await
                    .map_err(|error| admission_error(error.into()))?
                {
                    None => ProjectStateView::NotLocal,
                    Some(store) => {
                        let mut read = store
                            .snapshot()
                            .await
                            .map_err(|error| admission_error(error.into()))?;
                        let record = read
                            .record(&request.project_id.clone().into())
                            .await
                            .map_err(|error| admission_error(error.into()))?;
                        read.finish()
                            .await
                            .map_err(|error| admission_error(error.into()))?;
                        let Some(Record::Project(state)) = record else {
                            return Err(admission_error(
                                crate::ProjectAdmissionError::InvalidRecord(
                                    "local project state is missing".to_owned(),
                                ),
                            ));
                        };
                        ProjectStateView::Local(Box::new(state))
                    }
                };
                Ok(ProjectShowResult { project, state })
            }
            _ => Err(BoardError::invalid_field(
                "projectId",
                "is not a project directory entry",
            )),
        }
    }
    pub async fn workspace_show(
        &self,
        request: WorkspaceShowRequest,
    ) -> Result<WorkspaceShowResult, BoardError> {
        match self.fleet_record(request.workspace_id.into()).await? {
            Record::Workspace(workspace) => Ok(WorkspaceShowResult { workspace }),
            _ => Err(BoardError::invalid_field(
                "workspaceId",
                "is not a workspace",
            )),
        }
    }
    pub async fn board_show(
        &self,
        request: BoardShowRequest,
    ) -> Result<BoardShowResult, BoardError> {
        match self.fleet_record(request.board_id.into()).await? {
            Record::Board(board) => Ok(BoardShowResult { board }),
            _ => Err(BoardError::invalid_field("boardId", "is not a board")),
        }
    }
    pub async fn board_list(
        &self,
        request: BoardListRequest,
    ) -> Result<BoardListResult, BoardError> {
        let filter = fingerprint(&request)?;
        let page = self
            .fleet_list_page(
                message_board_storage::FleetListFilter::Boards {
                    workspace: request.workspace_id.as_ref(),
                    include_archived: request.include_archived,
                },
                request.page,
                "board_list",
                filter,
            )
            .await?;
        Ok(BoardListResult {
            page: Page {
                records: page
                    .records
                    .into_iter()
                    .map(|record| match record {
                        Record::Board(row) => Ok(row),
                        _ => Err(BoardError::board_unavailable()),
                    })
                    .collect::<Result<_, _>>()?,
                next_cursor: page.next_cursor,
            },
        })
    }
    pub async fn workspace_list(
        &self,
        request: WorkspaceListRequest,
    ) -> Result<WorkspaceListResult, BoardError> {
        if let Some(repository) = &request.repository {
            self.require_local_repository(repository)?;
        }
        let filter = fingerprint(&request)?;
        let page = self
            .fleet_list_page(
                message_board_storage::FleetListFilter::Workspaces {
                    repository: request.repository.as_ref(),
                },
                request.page,
                "workspace_list",
                filter,
            )
            .await?;
        Ok(WorkspaceListResult {
            page: Page {
                records: page
                    .records
                    .into_iter()
                    .map(|record| match record {
                        Record::Workspace(row) => Ok(row),
                        _ => Err(BoardError::board_unavailable()),
                    })
                    .collect::<Result<_, _>>()?,
                next_cursor: page.next_cursor,
            },
        })
    }
    pub async fn project_list(
        &self,
        request: ProjectListRequest,
    ) -> Result<ProjectListResult, BoardError> {
        let filter = fingerprint(&request)?;
        let page = self
            .fleet_list_page(
                message_board_storage::FleetListFilter::Projects {
                    board: &request.board_id,
                },
                request.page,
                "project_list",
                filter,
            )
            .await?;
        Ok(ProjectListResult {
            page: Page {
                records: page
                    .records
                    .into_iter()
                    .map(|record| match record {
                        Record::DirectoryEntry(row) => Ok(row),
                        _ => Err(BoardError::board_unavailable()),
                    })
                    .collect::<Result<_, _>>()?,
                next_cursor: page.next_cursor,
            },
        })
    }
    async fn project_record(
        &self,
        project: &ProjectId,
        reference: &RecordRef,
    ) -> Result<Record, BoardError> {
        let store = self
            .admission()?
            .registry
            .read_project(project)
            .await
            .map_err(|error| admission_error(error.into()))?
            .ok_or_else(BoardError::board_unavailable)?;
        let mut snapshot = store
            .snapshot()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let record = snapshot
            .record(reference)
            .await
            .map_err(|error| admission_error(error.into()))?;
        snapshot
            .finish()
            .await
            .map_err(|error| admission_error(error.into()))?;
        record.ok_or_else(|| super::missing_record(reference))
    }
    pub async fn post_show(&self, request: PostShowRequest) -> Result<PostShowResult, BoardError> {
        let reference = request.post_id.clone().into();
        let projects = self
            .admission()?
            .registry
            .post_projects(&request.post_id)
            .await
            .map_err(|error| admission_error(error.into()))?;
        let project = match projects.as_slice() {
            [] => return Err(super::missing_record(&reference)),
            [project] => project,
            _ => {
                return Err(BoardError::invalid_field(
                    "record",
                    "is ambiguous across projects",
                ));
            }
        };
        match self.project_record(project, &reference).await? {
            Record::Post(post) => Ok(PostShowResult(post)),
            _ => Err(BoardError::invalid_field("postId", "is not a post")),
        }
    }
    pub async fn thread_show(
        &self,
        request: ThreadShowRequest,
    ) -> Result<ThreadShowResult, BoardError> {
        let project = self.thread_project(&request.thread).await?;
        let store = self
            .admission()?
            .registry
            .read_project(&project)
            .await
            .map_err(|error| admission_error(error.into()))?
            .ok_or_else(BoardError::board_unavailable)?;
        let mut snapshot = store
            .snapshot()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let Some(Record::Thread(mut thread)) = snapshot
            .record(&request.thread.thread.clone().into())
            .await
            .map_err(|error| admission_error(error.into()))?
        else {
            return Err(BoardError::resource_not_found(ResourceIdentity::Thread {
                thread_id: request.thread.thread,
            }));
        };
        let participants = snapshot
            .participants(&request.thread.thread)
            .await
            .map_err(|error| admission_error(error.into()))?;
        thread.orchestrator = participants
            .iter()
            .find(|participant| {
                participant.is_open()
                    && participant.role
                        == RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator)
            })
            .map(|participant| OrchestratorHolder {
                identity: participant.identity.clone(),
                last_seen_position: participant.last_seen_position,
            });
        thread.implementer = participants
            .iter()
            .find(|participant| {
                participant.is_open()
                    && participant.role
                        == RecordedParticipantRole::Recorded(ParticipantRole::Implementer)
            })
            .map(|participant| ImplementerHolder {
                identity: participant.identity.clone(),
                last_seen_position: participant.last_seen_position,
            });
        let subject = self
            .thread_subject_batch(&mut snapshot, std::slice::from_ref(&thread.thread_id))
            .await?
            .take(&thread, request.include_redacted)?;
        let covered = snapshot.high_water();
        let watch_status = if let Some(reader) = request.reader.identity() {
            let starts_after = snapshot
                .effective_thread_watch(&request.thread.thread, reader)
                .await
                .map_err(|error| admission_error(error.into()))?;
            let history_bound = starts_after.unwrap_or(covered);
            let history_through = snapshot
                .thread_reply_high_water(&request.thread.thread, history_bound)
                .await
                .map_err(|error| admission_error(error.into()))?;
            let board = thread
                .envelope
                .scope
                .board
                .as_ref()
                .ok_or_else(BoardError::board_unavailable)?;
            let Record::Board(board) = self.fleet_record(board.clone().into()).await? else {
                return Err(BoardError::board_unavailable());
            };
            let board_status = match board.state {
                BoardState::Open => "active",
                BoardState::Closed => "closed",
                BoardState::Archiving | BoardState::Archived => "read-only",
            };
            let mut message = format!(
                "{} The board is {board_status}.",
                if starts_after.is_some() {
                    "Watching this thread."
                } else {
                    "This thread is not watched; use thread_watch to watch it."
                }
            );
            if history_through.is_some() {
                message.push_str(
                    " Use post_list with earlierUnwatchedRange for earlier thread posts.",
                );
            }
            Some(WatchStatus {
                watching: starts_after.is_some(),
                starts_after,
                earlier_unwatched_range: history_through.map(|through| HistoryRange {
                    scope: PostListScope::Thread {
                        thread: ThreadRef {
                            thread: request.thread.thread.clone(),
                            project: Some(project.clone()),
                        },
                    },
                    selection: PostSelection::Range {
                        from: ReadPosition::Project {
                            cursor: FeedCursor {
                                epoch: through.epoch,
                                position: FeedPosition::ZERO,
                            },
                        },
                        to: ReadPosition::Project { cursor: through },
                    },
                }),
                message,
            })
        } else {
            None
        };
        snapshot
            .finish()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let read_mark = self
            .finish_project_read(
                &project,
                ReadScope::Thread {
                    thread_id: request.thread.thread.clone(),
                },
                request.reader.identity(),
                None,
            )
            .await?;
        Ok(ThreadShowResult {
            project_id: project,
            read_mark,
            subject,
            thread,
            watch_status,
        })
    }
}
