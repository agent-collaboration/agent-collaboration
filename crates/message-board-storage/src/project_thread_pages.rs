//! Thread keys and watch starts are metadata; historical reply cursors keep their assigning epoch.
use crate::project_participant_io::{cursor, invalid};
use crate::{CursorValidity, ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::*;
use sqlx::Executor;
use sqlx_turso::Turso;

async fn thread_keys<TScope>(
    transaction: &mut TScope,
    after: &str,
    limit: i64,
) -> Result<Vec<ThreadId>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    sqlx_turso::query_scalar!(r#"SELECT thread_id AS "thread_id!: String" FROM threads WHERE thread_id>? ORDER BY thread_id LIMIT ?"#, after, limit).fetch_all(&mut *transaction).await?.into_iter().map(|id| ThreadId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed)).collect()
}

impl ProjectWriteTransaction {
    pub async fn thread_page_keys(
        &mut self,
        after: &str,
        limit: u64,
    ) -> Result<Vec<ThreadId>, ProjectStorageError> {
        let limit = i64::try_from(limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        thread_keys(&mut self.transaction, after, limit).await
    }
}

impl ProjectSnapshot {
    pub async fn thread_page_keys(
        &mut self,
        after: &str,
        limit: u64,
        watched_by: Option<&Identity>,
    ) -> Result<Vec<ThreadId>, ProjectStorageError> {
        let limit = i64::try_from(limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let Some(reader) = watched_by else {
            return thread_keys(&mut self.transaction, after, limit).await;
        };
        let Some(reader) = crate::project_actor_rows::actor_id(
            &mut self.transaction,
            &RecordedActor::declared(reader.clone()),
        )
        .await?
        else {
            return Ok(Vec::new());
        };
        let reader = reader.as_str();
        let rows = sqlx_turso::query!(r#"SELECT t.thread_id AS "thread_id!: String", w.start_epoch AS "start_epoch?: i64", w.start_position AS "start_position?: i64", COALESCE(w.active,0) AS "active!: i64", f.epoch AS "assigning_epoch?: i64" FROM threads t LEFT JOIN thread_watches w ON w.reader_actor=? AND w.thread_id=t.thread_id LEFT JOIN project_feed f ON f.position=w.start_position WHERE t.thread_id>? AND COALESCE(w.active,0)!=0 ORDER BY t.thread_id LIMIT ?"#, reader, after, limit).fetch_all(&mut self.transaction).await?;
        let mut threads = Vec::with_capacity(rows.len());
        let mut seen = std::collections::BTreeSet::new();
        for row in rows {
            let thread =
                ThreadId::try_from(row.thread_id).map_err(|_| ProjectStorageError::InvalidFeed)?;
            if row.active != 1 {
                return Err(invalid(&thread, "active"));
            }
            let start = cursor(
                row.start_epoch
                    .ok_or_else(|| invalid(&thread, "watch.startsAfter"))?,
                row.start_position
                    .ok_or_else(|| invalid(&thread, "watch.startsAfter"))?,
                &thread,
            )?;
            if start.position != FeedPosition::ZERO
                && row
                    .assigning_epoch
                    .and_then(|epoch| u64::try_from(epoch).ok())
                    != Some(start.epoch.get())
            {
                match self.cursor_validity(start).await? {
                    CursorValidity::Valid => {}
                    CursorValidity::Invalid => return Err(invalid(&thread, "watch.startsAfter")),
                    CursorValidity::Superseded { resume_from } => {
                        return Err(ProjectStorageError::CursorSuperseded { resume_from });
                    }
                }
            }
            if !seen.insert(thread.clone()) {
                return Err(ProjectStorageError::InvalidFeed);
            }
            threads.push(thread);
        }
        Ok(threads)
    }

    pub async fn thread_reply_high_water(
        &mut self,
        thread: &ThreadId,
        through: FeedCursor,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        match self.cursor_validity(through).await? {
            CursorValidity::Valid => {}
            CursorValidity::Invalid => return Err(ProjectStorageError::InvalidCursor),
            CursorValidity::Superseded { resume_from } => {
                return Err(ProjectStorageError::CursorSuperseded { resume_from });
            }
        }
        let thread = thread.as_str();
        let upper =
            i64::try_from(through.position.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let row = sqlx_turso::query!(r#"SELECT f.entry_id AS "entry_id!: String", f.position AS "position!: i64", f.epoch AS "epoch!: i64" FROM posts p CROSS JOIN project_feed f ON f.record_kind='post' AND f.record_id=p.post_id WHERE p.thread_id=? AND p.post_id!=? AND f.position<=? ORDER BY f.position DESC LIMIT 1"#, thread, thread, upper).fetch_optional(&mut self.transaction).await?;
        row.map(|row| {
            RecordId::try_from(row.entry_id).map_err(|_| ProjectStorageError::InvalidFeed)?;
            let cursor = FeedCursor {
                epoch: crate::project_record_codec::stored_counter(
                    row.epoch,
                    &self.project_id.clone().into(),
                    "epoch",
                )?,
                position: crate::project_record_codec::stored_counter(
                    row.position,
                    &self.project_id.clone().into(),
                    "position",
                )?,
            };
            if cursor.position == FeedPosition::ZERO {
                return Err(ProjectStorageError::InvalidFeed);
            }
            Ok(cursor)
        })
        .transpose()
    }

    pub async fn thread_subject_operand_keys(
        &mut self,
        threads: &[ThreadId],
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        if threads.is_empty() {
            return Ok(Vec::new());
        }
        let threads = serde_json::to_string(threads)?;
        let rows = sqlx_turso::query!(r#"SELECT thread_id AS "thread_id!: String", about AS "about!: String" FROM threads WHERE thread_id IN (SELECT value FROM json_each(?)) ORDER BY thread_id"#, threads).fetch_all(&mut self.transaction).await?;
        let mut keys = std::collections::BTreeSet::new();
        for row in rows {
            let reference = RecordRef {
                kind: RecordKind::Thread,
                id: RecordId::try_from(row.thread_id)
                    .map_err(|_| ProjectStorageError::InvalidFeed)?,
            };
            let about = crate::project_record_codec::stored_json::<ThreadSubject>(
                &row.about, &reference, "about",
            )?;
            let key: Option<RecordRef> = match about {
                ThreadSubject::Project { .. } => None,
                ThreadSubject::Workstream(id) => Some(id.into()),
                ThreadSubject::Task(id) => Some(id.into()),
                ThreadSubject::WorkTrailRow(id) => Some(id.into()),
                ThreadSubject::WorkspaceAdr(id) => Some(id.into()),
            };
            if let Some(key) = key {
                keys.insert((key.kind, key.id));
            }
        }
        Ok(keys
            .into_iter()
            .map(|(kind, id)| RecordRef { kind, id })
            .collect())
    }
}
