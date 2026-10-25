//! Watch choices share exact actor keys and retain their physical keys when updated.
use crate::project_actor_rows::actor_id;
use crate::project_record_codec::{require_one_row, stored_bool};
use crate::{ImportRowKind, ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::*;
use sqlx::Executor;
use sqlx_turso::Turso;

struct StoredWatch {
    watch_id: String,
    start_epoch: i64,
    start_position: i64,
    active: i64,
}

impl StoredWatch {
    fn validate(
        &self,
        reference: &RecordRef,
        high: FeedCursor,
    ) -> Result<FeedCursor, ProjectStorageError> {
        let invalid = |field| ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        };
        RecordId::try_from(self.watch_id.clone()).map_err(|_| invalid("watchId"))?;
        let cursor = FeedCursor {
            epoch: EpochNumber::try_from(
                u64::try_from(self.start_epoch).map_err(|_| invalid("watch.startsAfter"))?,
            )
            .map_err(|_| invalid("watch.startsAfter"))?,
            position: FeedPosition::try_from(
                u64::try_from(self.start_position).map_err(|_| invalid("watch.startsAfter"))?,
            )
            .map_err(|_| invalid("watch.startsAfter"))?,
        };
        if cursor.position > high.position {
            return Err(invalid("watch.startsAfter"));
        }
        stored_bool(self.active, reference, "watch.active")?;
        Ok(cursor)
    }
}

enum WatchState {
    Absent,
    Disabled,
    Active(FeedCursor),
}

async fn load_watch<TScope>(
    transaction: &mut TScope,
    reader: &Identity,
    thread: Option<&ThreadId>,
) -> Result<Option<StoredWatch>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let Some(actor) = actor_id(transaction, &RecordedActor::declared(reader.clone())).await? else {
        return Ok(None);
    };
    let actor = actor.as_str();
    Ok(match thread {
        Some(thread) => {
            let thread = thread.as_str();
            sqlx_turso::query_as!(StoredWatch, r#"SELECT watch_id AS "watch_id!: String", start_epoch AS "start_epoch!: i64", start_position AS "start_position!: i64", active AS "active!: i64" FROM thread_watches WHERE reader_actor=? AND thread_id=?"#, actor, thread)
                .fetch_optional(&mut *transaction).await?
        },
        None => sqlx_turso::query_as!(StoredWatch, r#"SELECT watch_id AS "watch_id!: String", start_epoch AS "start_epoch!: i64", start_position AS "start_position!: i64", active AS "active!: i64" FROM project_watches WHERE reader_actor=?"#, actor)
            .fetch_optional(&mut *transaction).await?,
    })
}

async fn watch_state<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    reader: &Identity,
    thread: Option<&ThreadId>,
) -> Result<WatchState, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let Some(row) = load_watch(transaction, reader, thread).await? else {
        return Ok(WatchState::Absent);
    };
    let reference: RecordRef =
        thread.map_or_else(|| project.clone().into(), |thread| thread.clone().into());
    let high = crate::project_store::checkpoint(transaction).await?;
    let cursor = row.validate(&reference, high)?;
    crate::project_epoch_reads::require_valid_cursor(transaction, cursor, high).await?;
    Ok(if stored_bool(row.active, &reference, "watch.active")? {
        WatchState::Active(cursor)
    } else {
        WatchState::Disabled
    })
}

async fn project_watch<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    reader: &Identity,
) -> Result<Option<FeedCursor>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    Ok(
        match watch_state(transaction, project, reader, None).await? {
            WatchState::Active(cursor) => Some(cursor),
            WatchState::Absent | WatchState::Disabled => None,
        },
    )
}

async fn thread_watch<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    thread: &ThreadId,
    reader: &Identity,
) -> Result<Option<FeedCursor>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    Ok(
        match watch_state(transaction, project, reader, Some(thread)).await? {
            WatchState::Active(cursor) => Some(cursor),
            WatchState::Absent | WatchState::Disabled => None,
        },
    )
}

async fn effective_thread_watch<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    thread: &ThreadId,
    reader: &Identity,
) -> Result<Option<FeedCursor>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    match watch_state(transaction, project, reader, Some(thread)).await? {
        WatchState::Active(cursor) => Ok(Some(cursor)),
        WatchState::Disabled | WatchState::Absent => Ok(None),
    }
}

impl ProjectSnapshot {
    pub async fn has_reader_interest(
        &mut self,
        reader: &Identity,
    ) -> Result<bool, ProjectStorageError> {
        let identity = serde_json::to_string(&RecordedActor::declared(reader.clone()))?;
        let present = sqlx_turso::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM actors a WHERE a.canonical_identity=? AND (EXISTS(SELECT 1 FROM read_marks m WHERE m.reader_actor=a.actor_id) OR EXISTS(SELECT 1 FROM project_watches w WHERE w.reader_actor=a.actor_id AND w.active=1) OR EXISTS(SELECT 1 FROM thread_watches w WHERE w.reader_actor=a.actor_id AND w.active=1))) AS "present!: i64""#, identity)
            .fetch_one(&mut self.transaction).await?;
        stored_bool(present, &self.project_id.clone().into(), "readerInterest")
    }
    pub async fn project_watch(
        &mut self,
        reader: &Identity,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        project_watch(&mut self.transaction, &self.project_id, reader).await
    }
    pub async fn thread_watch(
        &mut self,
        thread: &ThreadId,
        reader: &Identity,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        thread_watch(&mut self.transaction, &self.project_id, thread, reader).await
    }
    pub async fn effective_thread_watch(
        &mut self,
        thread: &ThreadId,
        reader: &Identity,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        effective_thread_watch(&mut self.transaction, &self.project_id, thread, reader).await
    }
}

impl ProjectWriteTransaction {
    pub async fn project_watch(
        &mut self,
        reader: &Identity,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        project_watch(&mut self.transaction, &self.project_id, reader).await
    }
    pub async fn thread_watch(
        &mut self,
        thread: &ThreadId,
        reader: &Identity,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        thread_watch(&mut self.transaction, &self.project_id, thread, reader).await
    }
    pub async fn effective_thread_watch(
        &mut self,
        thread: &ThreadId,
        reader: &Identity,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        effective_thread_watch(&mut self.transaction, &self.project_id, thread, reader).await
    }
    pub async fn thread_watch_exists(
        &mut self,
        thread: &ThreadId,
        reader: &Identity,
    ) -> Result<bool, ProjectStorageError> {
        Ok(!matches!(
            watch_state(
                &mut self.transaction,
                &self.project_id,
                reader,
                Some(thread)
            )
            .await?,
            WatchState::Absent
        ))
    }
    pub async fn set_project_watch(
        &mut self,
        reader: &Identity,
        starts_after: FeedCursor,
        active: bool,
    ) -> Result<(), ProjectStorageError> {
        self.set_watch(reader, None, starts_after, active).await
    }
    pub async fn set_thread_watch(
        &mut self,
        thread: &ThreadId,
        reader: &Identity,
        starts_after: FeedCursor,
        active: bool,
    ) -> Result<(), ProjectStorageError> {
        self.set_watch(reader, Some(thread), starts_after, active)
            .await
    }
    async fn set_watch(
        &mut self,
        reader: &Identity,
        thread: Option<&ThreadId>,
        starts_after: FeedCursor,
        active: bool,
    ) -> Result<(), ProjectStorageError> {
        let reference: RecordRef = thread.map_or_else(
            || self.project_id.clone().into(),
            |thread| thread.clone().into(),
        );
        let invalid = |field| ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        };
        let high = self.high_water().await?;
        crate::project_epoch_reads::require_valid_cursor(&mut self.transaction, starts_after, high)
            .await?;
        let previous = load_watch(&mut self.transaction, reader, thread).await?;
        if let Some(previous) = &previous {
            let cursor = previous.validate(&reference, high)?;
            crate::project_epoch_reads::require_valid_cursor(&mut self.transaction, cursor, high)
                .await?;
        }
        let declared = RecordedActor::declared(reader.clone());
        let actor = self.intern_actor(&declared).await?;
        let actor_key = actor.as_str();
        let epoch =
            i64::try_from(starts_after.epoch.get()).map_err(|_| invalid("watch.startsAfter"))?;
        let position =
            i64::try_from(starts_after.position.get()).map_err(|_| invalid("watch.startsAfter"))?;
        let changed = match (previous, thread) {
            (None, None) => {
                let canonical = serde_json::to_string(&declared)?;
                let key = self
                    .admission
                    .row_key(ImportRowKind::ProjectWatch, &[&canonical])?;
                let key = key.as_str();
                sqlx_turso::query!("INSERT INTO project_watches(watch_id, reader_actor, start_epoch, start_position, active) VALUES (?, ?, ?, ?, ?)", key, actor_key, epoch, position, active).execute(&mut self.transaction).await?
            }
            (None, Some(thread)) => {
                let canonical = serde_json::to_string(&declared)?;
                let thread = thread.as_str();
                let key = self
                    .admission
                    .row_key(ImportRowKind::ThreadWatch, &[&canonical, thread])?;
                let key = key.as_str();
                sqlx_turso::query!("INSERT INTO thread_watches(watch_id, reader_actor, thread_id, start_epoch, start_position, active) VALUES (?, ?, ?, ?, ?, ?)", key, actor_key, thread, epoch, position, active).execute(&mut self.transaction).await?
            }
            (Some(previous), None) => {
                let key = RecordId::try_from(previous.watch_id).map_err(|_| invalid("watchId"))?;
                let key = key.as_str();
                stored_bool(previous.active, &reference, "watch.active")?;
                sqlx_turso::query!("UPDATE project_watches SET start_epoch=CASE WHEN active=1 AND ?=1 THEN start_epoch ELSE ? END, start_position=CASE WHEN active=1 AND ?=1 THEN start_position ELSE ? END, active=? WHERE watch_id=? AND reader_actor=?", active, epoch, active, position, active, key, actor_key).execute(&mut self.transaction).await?
            }
            (Some(previous), Some(thread)) => {
                let key = RecordId::try_from(previous.watch_id).map_err(|_| invalid("watchId"))?;
                let key = key.as_str();
                let thread = thread.as_str();
                stored_bool(previous.active, &reference, "watch.active")?;
                sqlx_turso::query!("UPDATE thread_watches SET start_epoch=CASE WHEN active=1 OR ?=0 THEN start_epoch ELSE ? END, start_position=CASE WHEN active=1 OR ?=0 THEN start_position ELSE ? END, active=? WHERE watch_id=? AND reader_actor=? AND thread_id=?", active, epoch, active, position, active, key, actor_key, thread).execute(&mut self.transaction).await?
            }
        };
        require_one_row(changed.rows_affected(), reference)
    }
}
