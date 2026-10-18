//! Reader-first watch discovery never reads unrelated project contents.
use crate::{FeedPageBounds, FeedPageDirection, FeedPageKey, ProjectSnapshot, ProjectStorageError};
use message_board::{Identity, InboxReadMode};

pub struct ReaderTrackingMetadata {
    pub has_interest: bool,
    pub main_initialized: bool,
}
impl ProjectSnapshot {
    pub async fn inbox_scan_start(
        &mut self,
        reader: &Identity,
        selected: bool,
    ) -> Result<Option<message_board::FeedPosition>, ProjectStorageError> {
        let reader = serde_json::to_string(reader)?;
        let scope = serde_json::to_string(&message_board::ReadScope::Project {
            project_id: self.project_id.clone(),
        })?;
        let epoch = i64::try_from(self.high_water.epoch.get())
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let row = sqlx_turso::query_file!(
            "src/project_inbox_start.sql",
            reader,
            scope,
            epoch,
            selected
        )
        .fetch_one(&mut *self.transaction)
        .await?;
        row.position
            .map(|position| {
                let position = message_board::FeedPosition::try_from(
                    u64::try_from(position).map_err(|_| ProjectStorageError::InvalidFeed)?,
                )
                .map_err(|_| ProjectStorageError::InvalidFeed)?;
                if position > self.high_water.position {
                    return Err(ProjectStorageError::InvalidFeed);
                }
                Ok(position)
            })
            .transpose()
    }

    pub async fn reader_tracking_metadata(
        &mut self,
        reader: &Identity,
    ) -> Result<ReaderTrackingMetadata, ProjectStorageError> {
        let reader = serde_json::to_string(reader)?;
        let scope = serde_json::to_string(&message_board::ReadScope::Project {
            project_id: self.project_id.clone(),
        })?;
        let row = sqlx_turso::query!(r#"SELECT (EXISTS(SELECT 1 FROM read_marks WHERE reader = ?) OR EXISTS(SELECT 1 FROM project_watches WHERE reader = ? AND active = 1) OR EXISTS(SELECT 1 FROM thread_watches WHERE reader = ? AND active = 1)) AS "interest!: bool", EXISTS(SELECT 1 FROM read_marks WHERE reader = ? AND scope = ?) AS "initialized!: bool""#, reader, reader, reader, reader, scope).fetch_one(&mut *self.transaction).await?;
        Ok(ReaderTrackingMetadata {
            has_interest: row.interest,
            main_initialized: row.initialized,
        })
    }
    #[tracing::instrument(skip_all, fields(project = %self.project_id.as_str()))]
    pub async fn inbox_page_keys(
        &mut self,
        bounds: &FeedPageBounds,
        reader: &Identity,
        mode: InboxReadMode,
        selected: bool,
    ) -> Result<Vec<FeedPageKey>, ProjectStorageError> {
        let reader = serde_json::to_string(reader)?;
        let project = self.project_id.as_str();
        let unread = mode == InboxReadMode::Unread;
        let start =
            i64::try_from(bounds.after.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let end =
            i64::try_from(bounds.through.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let limit = i64::try_from(bounds.limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let sweep = bounds
            .sweep
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        macro_rules! decode_rows {
            ($rows:expr) => {
                $rows
                    .into_iter()
                    .map(|row| {
                        let key = self.feed_page_key(
                            row.position,
                            row.epoch,
                            row.record_kind,
                            row.record_id,
                            row.router_time,
                        )?;
                        if row.watch_active != 0 && row.watch_active != 1 {
                            return Err(ProjectStorageError::InvalidFeed);
                        }
                        for (epoch, position) in [
                            (row.watch_epoch, row.watch_position),
                            (row.mark_epoch, row.mark_position),
                        ] {
                            match (epoch, position) {
                                (None, None) => {}
                                (Some(epoch), Some(position)) => {
                                    let cursor = super::project_participant_io::cursor(
                                        epoch,
                                        position,
                                        &message_board::ThreadId::try_from(
                                            key.reference.id.as_str().to_owned(),
                                        )
                                        .map_err(|_| ProjectStorageError::InvalidFeed)?,
                                    )?;
                                    if cursor.epoch == self.high_water.epoch
                                        && cursor.position > self.high_water.position
                                    {
                                        return Err(ProjectStorageError::InvalidFeed);
                                    }
                                }
                                _ => return Err(ProjectStorageError::InvalidFeed),
                            }
                        }
                        Ok(key)
                    })
                    .collect()
            };
        }
        match bounds.direction {
            FeedPageDirection::Forward => decode_rows!(
                sqlx_turso::query_file!(
                    "src/project_inbox_page.sql",
                    reader,
                    selected,
                    unread,
                    project,
                    sweep,
                    start,
                    end,
                    limit
                )
                .fetch_all(&mut *self.transaction)
                .await?
            ),
            FeedPageDirection::Backward => decode_rows!(
                sqlx_turso::query_file!(
                    "src/project_inbox_page_backward.sql",
                    reader,
                    selected,
                    unread,
                    project,
                    sweep,
                    start,
                    end,
                    limit
                )
                .fetch_all(&mut *self.transaction)
                .await?
            ),
        }
    }
}
