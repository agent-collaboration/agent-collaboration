//! Thread candidates use the thread key and watch primary keys, without reading bodies.
use crate::project_participant_io::{cursor, invalid};
use crate::{ProjectSnapshot, ProjectStorageError};
use message_board::{Identity, ThreadId};

impl ProjectSnapshot {
    pub async fn thread_page_keys(
        &mut self,
        after: &str,
        limit: u64,
        watched_by: Option<&Identity>,
    ) -> Result<Vec<ThreadId>, ProjectStorageError> {
        let limit = i64::try_from(limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let Some(reader) = watched_by else {
            return sqlx_turso::query_scalar!(r#"SELECT thread_id AS "thread_id!: String" FROM threads WHERE thread_id > ? ORDER BY thread_id LIMIT ?"#, after, limit)
                .fetch_all(&mut *self.transaction).await?.into_iter()
                .map(|id| ThreadId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed)).collect();
        };
        let reader = serde_json::to_string(reader)?;
        let rows = sqlx_turso::query!(r#"SELECT t.thread_id AS "thread_id!: String", COALESCE(w.start_epoch, p.start_epoch) AS "start_epoch: i64", COALESCE(w.start_position, p.start_position) AS "start_position: i64", COALESCE(w.active, p.active, 0) AS "active!: i64" FROM threads t LEFT JOIN thread_watches w ON w.reader = ? AND w.thread_id = t.thread_id LEFT JOIN project_watches p ON p.reader = ? WHERE t.thread_id > ? AND COALESCE(w.active, p.active, 0) != 0 ORDER BY t.thread_id LIMIT ?"#, reader, reader, after, limit).fetch_all(&mut *self.transaction).await?;
        rows.into_iter()
            .map(|row| {
                let thread = ThreadId::try_from(row.thread_id)
                    .map_err(|_| ProjectStorageError::InvalidFeed)?;
                if row.active != 1 {
                    return Err(invalid(&thread, "active"));
                }
                let start = cursor(
                    row.start_epoch
                        .ok_or_else(|| invalid(&thread, "threadWatch.startsAfter"))?,
                    row.start_position
                        .ok_or_else(|| invalid(&thread, "threadWatch.startsAfter"))?,
                    &thread,
                )?;
                if start.epoch == self.high_water.epoch && start.position > self.high_water.position
                {
                    return Err(invalid(&thread, "threadWatch.startsAfter"));
                }
                Ok(thread)
            })
            .collect()
    }
}

impl ProjectSnapshot {
    /// The root is a main post; only replies can form earlier unwatched thread history.
    pub async fn thread_reply_high_water(
        &mut self,
        thread: &ThreadId,
        through: message_board::FeedCursor,
    ) -> Result<Option<message_board::FeedCursor>, ProjectStorageError> {
        if through.epoch != self.high_water.epoch {
            return Ok(None);
        }
        if through.position > self.high_water.position {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let thread = thread.as_str();
        let upper =
            i64::try_from(through.position.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let row = sqlx_turso::query!(r#"SELECT MAX(f.position) AS "position?: i64", MIN(f.epoch) AS "minimum_epoch?: i64", MAX(f.epoch) AS "maximum_epoch?: i64" FROM posts p CROSS JOIN project_feed f INDEXED BY feed_by_record ON f.record_kind = 'post' AND f.record_id = p.post_id WHERE p.thread_id = ? AND p.post_id != ? AND f.position <= ?"#, thread, thread, upper).fetch_one(&mut *self.transaction).await?;
        match (row.position, row.minimum_epoch, row.maximum_epoch) {
            (None, None, None) => Ok(None),
            (Some(position), Some(minimum), Some(maximum))
                if minimum == maximum
                    && u64::try_from(minimum).ok() == Some(through.epoch.get()) =>
            {
                let position = message_board::FeedPosition::try_from(
                    u64::try_from(position).map_err(|_| ProjectStorageError::InvalidFeed)?,
                )
                .map_err(|_| ProjectStorageError::InvalidFeed)?;
                if position == message_board::FeedPosition::ZERO {
                    return Err(ProjectStorageError::InvalidFeed);
                }
                Ok(Some(message_board::FeedCursor {
                    epoch: through.epoch,
                    position,
                }))
            }
            _ => Err(ProjectStorageError::InvalidFeed),
        }
    }
}

impl crate::ProjectSnapshot {
    pub async fn thread_subject_operand_keys(
        &mut self,
        threads: &[message_board::ThreadId],
    ) -> Result<Vec<message_board::RecordRef>, crate::ProjectStorageError> {
        if threads.is_empty() {
            return Ok(Vec::new());
        }
        let threads = serde_json::to_string(threads)?;
        let rows = sqlx_turso::query!(r#"SELECT DISTINCT s.subject_kind AS "subject_kind!: String", s.subject_id AS "subject_id!: String" FROM json_each(?) k CROSS JOIN thread_subjects s ON s.thread_id = CAST(k.value AS TEXT) WHERE s.subject_id IS NOT NULL ORDER BY s.subject_kind, s.subject_id"#, threads).fetch_all(&mut *self.transaction).await?;
        rows.into_iter()
            .map(|row| {
                let kind = match row.subject_kind.as_str() {
                    "workstream" => message_board::RecordKind::Workstream,
                    "task" => message_board::RecordKind::Task,
                    "workTrailRow" => message_board::RecordKind::WorkTrailRow,
                    "adr" => message_board::RecordKind::Adr,
                    _ => return Err(crate::ProjectStorageError::InvalidFeed),
                };
                Ok(message_board::RecordRef {
                    kind,
                    id: message_board::RecordId::try_from(row.subject_id)
                        .map_err(|_| crate::ProjectStorageError::InvalidFeed)?,
                })
            })
            .collect()
    }
}
