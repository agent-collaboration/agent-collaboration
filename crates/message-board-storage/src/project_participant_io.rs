//! Native conversation intervals and interest, independent of authority assignments.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{
    EpochNumber, FeedContent, FeedCursor, FeedEntry, FeedPosition, Identity, Participant,
    ParticipantRole, ProjectId, Record, RecordPosition, ThreadId,
};

pub(super) fn invalid(thread: &ThreadId, field: &'static str) -> ProjectStorageError {
    ProjectStorageError::InvalidRecord {
        record: thread.clone().into(),
        field,
    }
}
pub(super) fn role_name(role: ParticipantRole) -> &'static str {
    match role {
        ParticipantRole::Orchestrator => "orchestrator",
        ParticipantRole::Implementer => "implementer",
        ParticipantRole::Advisor => "advisor",
        ParticipantRole::Reviewer => "reviewer",
        ParticipantRole::Participant => "participant",
    }
}
pub(super) fn cursor(
    epoch: i64,
    position: i64,
    thread: &ThreadId,
) -> Result<FeedCursor, ProjectStorageError> {
    Ok(FeedCursor {
        epoch: EpochNumber::try_from(u64::try_from(epoch).map_err(|_| invalid(thread, "epoch"))?)
            .map_err(|_| invalid(thread, "epoch"))?,
        position: FeedPosition::try_from(
            u64::try_from(position).map_err(|_| invalid(thread, "position"))?,
        )
        .map_err(|_| invalid(thread, "position"))?,
    })
}
async fn participants(
    connection: &mut sqlx_turso::TursoConnection,
    project: &ProjectId,
    thread: &ThreadId,
) -> Result<Vec<Participant>, ProjectStorageError> {
    let mut batch = super::project_participant_batch::participant_batch(
        connection,
        std::slice::from_ref(thread),
    )
    .await?;
    let participants = batch.take(thread)?;
    let by_thread = std::collections::BTreeMap::from([(thread.clone(), participants)]);
    super::project_participant_batch::validate_participant_anchors(connection, project, &by_thread)
        .await?;
    Ok(by_thread.into_values().next().unwrap_or_default())
}
pub(super) fn validate_participant_entry(
    project: &ProjectId,
    thread: &ThreadId,
    cursor: FeedCursor,
    entry: &str,
) -> Result<(), ProjectStorageError> {
    tracing::trace!(target: "message_board_storage::decode", decode_kind = "feed");
    let decoded: FeedEntry =
        serde_json::from_str(entry).map_err(|_| invalid(thread, "participant.position"))?;
    crate::project_record_io::validate_canonical_project_text(entry, &decoded, &decoded.record)?;
    let entry = decoded;
    let belongs = entry.epoch == cursor.epoch
        && entry.position == cursor.position
        && match entry.content {
            FeedContent::Shown(record)
                if record.reference() == entry.record
                    && record.envelope().position == RecordPosition::Project(cursor)
                    && record.envelope().scope.project.as_ref() == Some(project) =>
            {
                match *record {
                    Record::Thread(row) => row.thread_id == *thread,
                    Record::Post(row) => {
                        row.post_id.as_str() == thread.as_str()
                            || matches!(row.placement,message_board::Placement::Thread {thread_id} if thread_id == *thread)
                    }
                    _ => false,
                }
            }
            FeedContent::Shown(_) | FeedContent::Redacted => false,
        };
    if !belongs {
        return Err(invalid(thread, "participant.position"));
    }
    Ok(())
}
enum StoredThreadWatch {
    Absent,
    Disabled,
    Active(FeedCursor),
}
async fn stored_thread_watch(
    connection: &mut sqlx_turso::TursoConnection,
    thread: &ThreadId,
    reader: &Identity,
) -> Result<StoredThreadWatch, ProjectStorageError> {
    let reader = serde_json::to_string(reader)?;
    let id = thread.as_str();
    let row=sqlx_turso::query!(r#"SELECT start_epoch AS "start_epoch!: i64", start_position AS "start_position!: i64", active AS "active!: i64" FROM thread_watches WHERE thread_id = ? AND reader = ?"#,id,reader).fetch_optional(&mut *connection).await?;
    match row {
        None => Ok(StoredThreadWatch::Absent),
        Some(row) => {
            let through = cursor(row.start_epoch, row.start_position, thread)?;
            let high_water = crate::project_store::checkpoint(connection).await?;
            if through.epoch == high_water.epoch && through.position > high_water.position {
                return Err(invalid(thread, "threadWatch.startsAfter"));
            }
            match row.active {
                0 => Ok(StoredThreadWatch::Disabled),
                1 => Ok(StoredThreadWatch::Active(through)),
                _ => Err(invalid(thread, "active")),
            }
        }
    }
}
async fn thread_watch(
    connection: &mut sqlx_turso::TursoConnection,
    thread: &ThreadId,
    reader: &Identity,
) -> Result<Option<FeedCursor>, ProjectStorageError> {
    match stored_thread_watch(connection, thread, reader).await? {
        StoredThreadWatch::Active(cursor) => Ok(Some(cursor)),
        StoredThreadWatch::Absent | StoredThreadWatch::Disabled => Ok(None),
    }
}

impl ProjectSnapshot {
    pub async fn effective_thread_watch(
        &mut self,
        thread: &ThreadId,
        reader: &Identity,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        match stored_thread_watch(&mut self.transaction, thread, reader).await? {
            StoredThreadWatch::Active(cursor) => Ok(Some(cursor)),
            StoredThreadWatch::Disabled => Ok(None),
            StoredThreadWatch::Absent => self.project_watch(reader).await,
        }
    }

    pub async fn participants(
        &mut self,
        thread: &ThreadId,
    ) -> Result<Vec<Participant>, ProjectStorageError> {
        participants(&mut self.transaction, &self.project_id, thread).await
    }
    pub async fn thread_watch(
        &mut self,
        thread: &ThreadId,
        reader: &Identity,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        thread_watch(&mut self.transaction, thread, reader).await
    }
}
impl ProjectWriteTransaction {
    pub async fn thread_watch_exists(
        &mut self,
        thread: &ThreadId,
        reader: &Identity,
    ) -> Result<bool, ProjectStorageError> {
        let reader = serde_json::to_string(reader)?;
        let id = thread.as_str();
        let count = sqlx_turso::query_scalar!(
            r#"SELECT COUNT(*) AS "row_count!: i64" FROM thread_watches WHERE reader = ? AND thread_id = ?"#,
            reader,
            id
        )
        .fetch_one(&mut *self.transaction)
        .await?;
        match count {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(invalid(thread, "threadWatch")),
        }
    }

    pub async fn effective_thread_watch(
        &mut self,
        thread: &ThreadId,
        reader: &Identity,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        match stored_thread_watch(&mut self.transaction, thread, reader).await? {
            StoredThreadWatch::Active(cursor) => Ok(Some(cursor)),
            StoredThreadWatch::Disabled => Ok(None),
            StoredThreadWatch::Absent => self.project_watch(reader).await,
        }
    }

    pub async fn participants(
        &mut self,
        thread: &ThreadId,
    ) -> Result<Vec<Participant>, ProjectStorageError> {
        participants(&mut self.transaction, &self.project_id, thread).await
    }
    pub async fn thread_watch(
        &mut self,
        thread: &ThreadId,
        reader: &Identity,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        thread_watch(&mut self.transaction, thread, reader).await
    }
    pub async fn put_participant(
        &mut self,
        thread: &ThreadId,
        participant: &Participant,
    ) -> Result<(), ProjectStorageError> {
        let payload = serde_json::to_string(participant)?;
        let _: Participant =
            serde_json::from_str(&payload).map_err(|_| invalid(thread, "participant"))?;
        let reader = serde_json::to_string(&participant.identity)?;
        let id = thread.as_str();
        let epoch = i64::try_from(participant.joined_at_position.epoch.get())
            .map_err(|_| invalid(thread, "epoch"))?;
        let position = i64::try_from(participant.joined_at_position.position.get())
            .map_err(|_| invalid(thread, "position"))?;
        let role = role_name(participant.role);
        let active = participant.is_open();
        let row_count=sqlx_turso::query_scalar!(r#"SELECT COUNT(*) AS "row_count!: i64" FROM thread_participants WHERE reader = ? AND thread_id = ? AND joined_epoch = ? AND joined_position = ?"#,reader,id,epoch,position).fetch_one(&mut *self.transaction).await?;
        match row_count {
            0 => {
                sqlx_turso::query!("INSERT INTO thread_participants(reader, thread_id, joined_epoch, joined_position, role, active, payload) VALUES (?, ?, ?, ?, ?, ?, ?)",reader,id,epoch,position,role,active,payload).execute(&mut *self.transaction).await?;
            }
            1 => {
                sqlx_turso::query!("UPDATE thread_participants SET role = ?, active = ?, payload = ? WHERE reader = ? AND thread_id = ? AND joined_epoch = ? AND joined_position = ?",role,active,payload,reader,id,epoch,position).execute(&mut *self.transaction).await?;
            }
            _ => return Err(ProjectStorageError::InvalidFeed),
        }
        Ok(())
    }
    pub async fn set_thread_watch(
        &mut self,
        thread: &ThreadId,
        reader: &Identity,
        starts_after: FeedCursor,
        active: bool,
    ) -> Result<(), ProjectStorageError> {
        let reader = serde_json::to_string(reader)?;
        let id = thread.as_str();
        let epoch =
            i64::try_from(starts_after.epoch.get()).map_err(|_| invalid(thread, "epoch"))?;
        let position =
            i64::try_from(starts_after.position.get()).map_err(|_| invalid(thread, "position"))?;
        let row_count=sqlx_turso::query_scalar!(r#"SELECT COUNT(*) AS "row_count!: i64" FROM thread_watches WHERE reader = ? AND thread_id = ?"#,reader,id).fetch_one(&mut *self.transaction).await?;
        match row_count {
            0 => {
                sqlx_turso::query!("INSERT INTO thread_watches(reader, thread_id, start_epoch, start_position, active) VALUES (?, ?, ?, ?, ?)",reader,id,epoch,position,active).execute(&mut *self.transaction).await?;
            }
            1 => {
                sqlx_turso::query!("UPDATE thread_watches SET start_epoch = CASE WHEN active = 1 OR ? = 0 THEN start_epoch ELSE ? END, start_position = CASE WHEN active = 1 OR ? = 0 THEN start_position ELSE ? END, active = ? WHERE reader = ? AND thread_id = ?",active,epoch,active,position,active,reader,id).execute(&mut *self.transaction).await?;
            }
            _ => return Err(ProjectStorageError::InvalidFeed),
        }
        Ok(())
    }
}
