//! Replicated reader marks have exact actor keys and no activity placement or origin.
use crate::project_actor_rows::{actor_id, decode_actor};
use crate::project_record_codec::{decode_record_time, encode_record_time, require_one_row};
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{
    BoardId, FeedCursor, Identity, ProjectId, ReadMark, ReadScope, Record, RecordEnvelope,
    RecordId, RecordKind, RecordPlacement, RecordRef, RecordScope, RecordTime, RecordedActor,
    RequestId, ThreadId, WorkspaceId,
};
use sqlx::Executor;
use sqlx_turso::Turso;

struct StoredReadMark {
    mark: ReadMark,
    normalized: bool,
}

pub(crate) fn scope_key(scope: &ReadScope) -> (&'static str, &str) {
    match scope {
        ReadScope::Project { project_id } => ("project", project_id.as_str()),
        ReadScope::Thread { thread_id } => ("thread", thread_id.as_str()),
    }
}

pub(crate) struct StoredReadMarkRow {
    mark_id: String,
    reader_actor: String,
    canonical_identity: Option<String>,
    request_id: Option<String>,
    router_time: Option<String>,
    time_basis: String,
    scope_kind: String,
    scope_id: String,
    through_epoch: i64,
    through_position: i64,
    workspace_id: String,
    board_id: String,
}

impl StoredReadMarkRow {
    pub(crate) fn reference(&self) -> Result<RecordRef, ProjectStorageError> {
        Ok(RecordRef {
            kind: RecordKind::ReadMark,
            id: RecordId::try_from(self.mark_id.clone())
                .map_err(|_| ProjectStorageError::InvalidFeed)?,
        })
    }
    pub(crate) fn decode(self, project: &ProjectId) -> Result<ReadMark, ProjectStorageError> {
        let reference = self.reference()?;
        let invalid = |field| ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        };
        let actor = decode_actor(
            &self.reader_actor,
            &self
                .canonical_identity
                .ok_or_else(|| invalid("readerActor"))?,
        )?;
        let reader = actor
            .declared_identity()
            .ok_or_else(|| invalid("readerActor"))?;
        let scope = match self.scope_kind.as_str() {
            "project" if self.scope_id == project.as_str() => ReadScope::Project {
                project_id: project.clone(),
            },
            "thread" => ReadScope::Thread {
                thread_id: crate::project_record_codec::stored_text(
                    self.scope_id,
                    &reference,
                    "scope",
                    ThreadId::as_str,
                )?,
            },
            _ => return Err(invalid("scope")),
        };
        Ok(ReadMark {
            envelope: RecordEnvelope {
                id: reference.id.clone(),
                scope: RecordScope {
                    workspace: crate::project_record_codec::stored_text(
                        self.workspace_id,
                        &reference,
                        "workspace",
                        WorkspaceId::as_str,
                    )?,
                    board: Some(crate::project_record_codec::stored_text(
                        self.board_id,
                        &reference,
                        "board",
                        BoardId::as_str,
                    )?),
                    project: Some(project.clone()),
                },
                actor,
                request: self
                    .request_id
                    .map(RequestId::try_from)
                    .transpose()
                    .map_err(|_| invalid("request"))?,
                position: RecordPlacement::Unsequenced,
                time: decode_record_time(self.router_time, &self.time_basis, &reference)?,
            },
            reader,
            scope,
            through: FeedCursor {
                epoch: crate::project_record_codec::stored_counter(
                    self.through_epoch,
                    &reference,
                    "readMark.through",
                )?,
                position: crate::project_record_codec::stored_counter(
                    self.through_position,
                    &reference,
                    "readMark.through",
                )?,
            },
        })
    }
}

async fn read_mark<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    reader: &Identity,
    scope: &ReadScope,
) -> Result<Option<StoredReadMark>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let declared = RecordedActor::declared(reader.clone());
    let Some(reader_id) = actor_id(transaction, &declared).await? else {
        return Ok(None);
    };
    let reader_key = reader_id.as_str();
    let project_key = project.as_str();
    let (scope_kind, scope_id) = scope_key(scope);
    let row = sqlx_turso::query_as!(StoredReadMarkRow, r#"
SELECT m.mark_id AS "mark_id!: String", m.reader_actor AS "reader_actor!: String", a.canonical_identity AS "canonical_identity?: String",
    m.request_id AS "request_id?: String", m.router_time AS "router_time?: String", m.time_basis AS "time_basis!: String",
    m.scope_kind AS "scope_kind!: String", m.scope_id AS "scope_id!: String",
    m.through_epoch AS "through_epoch!: i64", m.through_position AS "through_position!: i64",
    p.workspace_id AS "workspace_id!: String", p.board_id AS "board_id!: String"
FROM read_marks m CROSS JOIN project_state p LEFT JOIN actors a ON a.actor_id=m.reader_actor
WHERE m.reader_actor=? AND m.scope_kind=? AND m.scope_id=? AND p.project_id=?
"#, reader_key, scope_kind, scope_id, project_key).fetch_optional(&mut *transaction).await?;
    let Some(row) = row else { return Ok(None) };
    let mut mark = row.decode(project)?;
    if mark.reader != *reader || mark.scope != *scope {
        return Err(ProjectStorageError::InvalidFeed);
    }
    let high = crate::project_store::checkpoint(transaction).await?;
    let normalized =
        match crate::project_epoch_reads::cursor_validity(transaction, mark.through, high).await? {
            crate::CursorValidity::Valid => false,
            crate::CursorValidity::Superseded { resume_from } => {
                mark.through = resume_from;
                true
            }
            crate::CursorValidity::Invalid => {
                return Err(ProjectStorageError::InvalidRecord {
                    record: RecordRef {
                        kind: RecordKind::ReadMark,
                        id: mark.envelope.id.clone(),
                    },
                    field: "readMark.through",
                });
            }
        };
    Ok(Some(StoredReadMark { mark, normalized }))
}

pub(crate) async fn reader_mark_records<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    keys: &[&str],
) -> Result<Vec<StoredReadMarkRow>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let project = project.as_str();
    let keys = serde_json::to_string(keys)?;
    Ok(sqlx_turso::query_as!(StoredReadMarkRow, r#"
SELECT m.mark_id AS "mark_id!: String", m.reader_actor AS "reader_actor!: String", a.canonical_identity AS "canonical_identity?: String",
    m.request_id AS "request_id?: String", m.router_time AS "router_time?: String", m.time_basis AS "time_basis!: String",
    m.scope_kind AS "scope_kind!: String", m.scope_id AS "scope_id!: String",
    m.through_epoch AS "through_epoch!: i64", m.through_position AS "through_position!: i64",
    p.workspace_id AS "workspace_id!: String", p.board_id AS "board_id!: String"
FROM read_marks m CROSS JOIN project_state p LEFT JOIN actors a ON a.actor_id=m.reader_actor
WHERE p.project_id=? AND m.mark_id IN (SELECT value FROM json_each(?)) ORDER BY m.mark_id
"#, project, keys).fetch_all(&mut *transaction).await?)
}

impl ProjectSnapshot {
    pub async fn read_mark(
        &mut self,
        reader: &Identity,
        scope: &ReadScope,
    ) -> Result<Option<ReadMark>, ProjectStorageError> {
        Ok(
            read_mark(&mut self.transaction, &self.project_id, reader, scope)
                .await?
                .map(|stored| stored.mark),
        )
    }
}

impl ProjectWriteTransaction {
    pub async fn normalize_read_mark(
        &mut self,
        reader: &Identity,
        scope: &ReadScope,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(Option<ReadMark>, bool), ProjectStorageError> {
        let Some(mut stored) =
            read_mark(&mut self.transaction, &self.project_id, reader, scope).await?
        else {
            return Ok((None, false));
        };
        if stored.normalized {
            stored.mark.envelope.time = RecordTime::Router(at);
            self.write_reader_mark(stored.mark.clone()).await?;
        }
        Ok((Some(stored.mark), stored.normalized))
    }

    pub async fn read_mark(
        &mut self,
        reader: &Identity,
        scope: &ReadScope,
    ) -> Result<Option<ReadMark>, ProjectStorageError> {
        Ok(
            read_mark(&mut self.transaction, &self.project_id, reader, scope)
                .await?
                .map(|stored| stored.mark),
        )
    }

    pub async fn write_reader_mark(
        &mut self,
        mut mark: ReadMark,
    ) -> Result<(), ProjectStorageError> {
        let record = Record::ReadMark(mark.clone());
        crate::project_record_io::validate_project_record(&record, &self.project_id)?;
        let reference = record.reference();
        let invalid = |field| ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        };
        if mark.envelope.position != RecordPlacement::Unsequenced
            || mark.envelope.actor != RecordedActor::declared(mark.reader.clone())
        {
            return Err(invalid("readerActor"));
        }
        if !self.admission.admits_time(mark.envelope.time) {
            return Err(invalid("time"));
        }
        self.validate_record_scope(&mark.envelope).await?;
        let high = self.high_water().await?;
        crate::project_epoch_reads::require_valid_cursor(&mut self.transaction, mark.through, high)
            .await?;
        let previous = read_mark(
            &mut self.transaction,
            &self.project_id,
            &mark.reader,
            &mark.scope,
        )
        .await?;
        if let Some(previous) = &previous {
            if previous.mark.envelope.id != mark.envelope.id {
                return Err(invalid("id"));
            }
            if mark.through.position <= previous.mark.through.position {
                if !previous.normalized {
                    return Ok(());
                }
                mark.through = previous.mark.through;
            }
        }
        let reader = self.intern_actor(&mark.envelope.actor).await?;
        let reader = reader.as_str();
        let (scope_kind, scope_id) = scope_key(&mark.scope);
        let id = mark.envelope.id.as_str();
        let request = mark.envelope.request.as_ref().map(RequestId::as_str);
        let (time, basis) = encode_record_time(mark.envelope.time);
        let epoch =
            i64::try_from(mark.through.epoch.get()).map_err(|_| invalid("readMark.through"))?;
        let position =
            i64::try_from(mark.through.position.get()).map_err(|_| invalid("readMark.through"))?;
        let changed = match previous {
            None => sqlx_turso::query!(
                "INSERT INTO read_marks(mark_id, reader_actor, request_id, router_time, time_basis, scope_kind, scope_id, through_epoch, through_position) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                id, reader, request, time, basis, scope_kind, scope_id, epoch, position
            ).execute(&mut self.transaction).await?,
            Some(_) => sqlx_turso::query!(
                "UPDATE read_marks SET request_id=?, router_time=?, time_basis=?, through_epoch=?, through_position=? WHERE mark_id=? AND reader_actor=? AND scope_kind=? AND scope_id=?",
                request, time, basis, epoch, position, id, reader, scope_kind, scope_id
            ).execute(&mut self.transaction).await?,
        };
        require_one_row(changed.rows_affected(), reference)
    }
}
