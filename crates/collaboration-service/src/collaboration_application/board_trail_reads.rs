//! Forward reads cover scanned positions; registers complete a frozen descending range.
use super::board_paging::invalid_cursor;
use super::board_trail_pages::trail_read_prefix;
use super::{BoardOperations, admission_error};
use message_board::{
    BoardError, BoardReadReply, DecisionPage, FeedCursor, FeedPosition, FeedRead, FeedReadRequest,
    ReadMarkOutcome, ReadScope, TrailPage, WorkTrailReadResult, WorkTrailSelection,
    WorkTrailShowRequest,
};
use message_board_storage::{FeedPageBounds, FeedPageDirection, TrailPageFilter};
impl BoardOperations<'_> {
    pub async fn feed_read(
        &self,
        request: FeedReadRequest,
    ) -> Result<BoardReadReply<FeedRead>, BoardError> {
        let store = self
            .admission()?
            .registry
            .read_project(&request.project_id)
            .await
            .map_err(|error| admission_error(error.into()))?
            .ok_or_else(BoardError::board_unavailable)?;
        let mut read = store
            .snapshot()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let result = read
            .feed_after(
                request.after,
                u64::from(request.limit.get()),
                request.include_redacted,
            )
            .await
            .map_err(|error| admission_error(error.into()))?;
        read.finish()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let read_mark = match &result {
            FeedRead::Entries {
                covered_through, ..
            } => {
                self.finish_project_read(
                    &request.project_id,
                    ReadScope::Project {
                        project_id: request.project_id.clone(),
                    },
                    request.reader.identity(),
                    Some(*covered_through),
                )
                .await?
            }
            FeedRead::CursorSuperseded { .. } => request
                .reader
                .identity()
                .map(|_| ReadMarkOutcome::Unchanged {}),
        };
        Ok(BoardReadReply {
            value: result,
            read_mark,
        })
    }
    pub async fn work_trail_show(
        &self,
        request: WorkTrailShowRequest,
    ) -> Result<BoardReadReply<WorkTrailReadResult>, BoardError> {
        let store = self
            .admission()?
            .registry
            .read_project(&request.project_id)
            .await
            .map_err(|error| admission_error(error.into()))?
            .ok_or_else(BoardError::board_unavailable)?;
        let mut read = store
            .snapshot()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let high_water = read.high_water();
        let result = match request.selection {
            WorkTrailSelection::Forward { after } => {
                let validity = read
                    .cursor_validity(after)
                    .await
                    .map_err(|error| admission_error(error.into()))?;
                if let message_board_storage::CursorValidity::Superseded { resume_from } = validity
                {
                    WorkTrailReadResult::CursorSuperseded { resume_from }
                } else {
                    if validity != message_board_storage::CursorValidity::Valid
                        || after.position > high_water.position
                    {
                        return Err(admission_error(
                            message_board_storage::ProjectStorageError::InvalidFeed.into(),
                        ));
                    }
                    let top = FeedPosition::try_from(
                        after
                            .position
                            .get()
                            .saturating_add(u64::from(request.limit.get()))
                            .min(high_water.position.get()),
                    )
                    .map_err(|_| invalid_cursor())?;
                    let prefix = trail_read_prefix(
                        &mut read,
                        FeedPageBounds {
                            after: after.position,
                            through: top,
                            direction: FeedPageDirection::Forward,
                            limit: u64::from(request.limit.get()) + 1,
                            sweep: None,
                        },
                        TrailPageFilter {
                            subject: request.subject.as_ref(),
                            tags: &request.tags,
                            include_redacted: request.include_redacted,
                            decisions_only: false,
                        },
                    )
                    .await?;
                    let covered_through = if prefix.more {
                        prefix
                            .last_returned
                            .ok_or_else(BoardError::board_unavailable)?
                    } else {
                        read.feed_cursor_or_start(top)
                            .await
                            .map_err(|error| admission_error(error.into()))?
                    };
                    WorkTrailReadResult::Forward(TrailPage {
                        rows: prefix.rows,
                        covered_through,
                        high_water,
                    })
                }
            }
            WorkTrailSelection::Decisions {
                after,
                before,
                range_top,
            } => {
                let lower = after.unwrap_or(FeedCursor {
                    epoch: high_water.epoch,
                    position: FeedPosition::ZERO,
                });
                let upper = before.unwrap_or(high_water);
                let top = range_top.unwrap_or(upper);
                if lower.position > upper.position
                    || upper.position > top.position
                    || top.position > high_water.position
                {
                    return Err(invalid_cursor());
                }
                for cursor in [lower, upper, top] {
                    if read
                        .cursor_validity(cursor)
                        .await
                        .map_err(|error| admission_error(error.into()))?
                        != message_board_storage::CursorValidity::Valid
                    {
                        return Err(invalid_cursor());
                    }
                }
                let prefix = trail_read_prefix(
                    &mut read,
                    FeedPageBounds {
                        after: lower.position,
                        through: upper.position,
                        direction: FeedPageDirection::Backward,
                        limit: u64::from(request.limit.get()) + 1,
                        sweep: None,
                    },
                    TrailPageFilter {
                        subject: request.subject.as_ref(),
                        tags: &request.tags,
                        include_redacted: request.include_redacted,
                        decisions_only: true,
                    },
                )
                .await?;
                let rows = prefix.rows;
                let more = prefix.more;
                let older_than = if more {
                    let Some(position) = rows
                        .last()
                        .ok_or_else(invalid_cursor)?
                        .envelope
                        .position
                        .project_cursor()
                    else {
                        return Err(invalid_cursor());
                    };
                    Some(
                        read.feed_cursor_or_start(
                            FeedPosition::try_from(position.position.get().saturating_sub(1))
                                .map_err(|_| invalid_cursor())?,
                        )
                        .await
                        .map_err(|error| admission_error(error.into()))?,
                    )
                } else {
                    None
                };
                WorkTrailReadResult::Decisions(DecisionPage {
                    rows,
                    older_than,
                    range_top: top,
                    covered_through: (!more).then_some(top),
                })
            }
        };
        read.finish()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let covered = match &result {
            WorkTrailReadResult::Forward(page) => Some(page.covered_through),
            WorkTrailReadResult::Decisions(page) => page.covered_through,
            WorkTrailReadResult::CursorSuperseded { .. } => None,
        };
        let read_mark = self
            .finish_project_read(
                &request.project_id,
                ReadScope::Project {
                    project_id: request.project_id.clone(),
                },
                request.reader.identity(),
                covered,
            )
            .await?;
        Ok(BoardReadReply {
            value: result,
            read_mark,
        })
    }
}
