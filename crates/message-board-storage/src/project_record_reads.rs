//! Snapshot-consistent typed records and independent task-link rows.
use crate::{
    ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction,
    project_record_io::{
        project_record_projection, read_record, record_kind_tag, validate_canonical_project_text,
        validate_project_record,
    },
};
use message_board::{
    Identity, LinkChange, ProjectId, ReadScope, Record, RecordId, RecordKind, RecordRef, TaskId,
    TaskLink, TaskLinkKind,
};
use sqlx_turso::TursoConnection;

async fn records_of_kind(
    connection: &mut TursoConnection,
    project: &ProjectId,
    kind: RecordKind,
) -> Result<Vec<Record>, ProjectStorageError> {
    let tag = record_kind_tag(kind);
    let rows = sqlx_turso::query!(r#"SELECT id AS "id!: String", payload AS "payload!: String" FROM record_identities WHERE kind = ? ORDER BY id"#, tag).fetch_all(&mut *connection).await?;
    let mut records = Vec::with_capacity(rows.len());
    for row in rows {
        tracing::trace!(target: "message_board_storage::decode", decode_kind = "record");
        let id = RecordId::try_from(row.id).map_err(|_| ProjectStorageError::InvalidRecord {
            record: project.clone().into(),
            field: "id",
        })?;
        let reference = RecordRef { kind, id };
        let record: Record =
            serde_json::from_str(&row.payload).map_err(|_| ProjectStorageError::InvalidRecord {
                record: reference.clone(),
                field: "payload",
            })?;
        if record.reference() != reference {
            return Err(ProjectStorageError::InvalidRecord {
                record: reference,
                field: "id",
            });
        }
        validate_canonical_project_text(&row.payload, &record, &reference)?;
        validate_project_record(&record, project)?;
        records.push(project_record_projection(connection, record).await?);
    }
    Ok(records)
}

async fn read_task_links(
    connection: &mut TursoConnection,
    project: &ProjectId,
) -> Result<Vec<TaskLink>, ProjectStorageError> {
    let rows = sqlx_turso::query!(r#"SELECT from_task AS "from_task!: String", link_kind AS "link_kind!: String", to_task AS "to_task!: String" FROM task_links ORDER BY from_task, link_kind, to_task"#).fetch_all(connection).await?;
    rows.into_iter()
        .map(|row| {
            let invalid = || ProjectStorageError::InvalidRecord {
                record: project.clone().into(),
                field: "taskLink",
            };
            let from = TaskId::try_from(row.from_task).map_err(|_| invalid())?;
            let to = TaskId::try_from(row.to_task).map_err(|_| invalid())?;
            let kind = match row.link_kind.as_str() {
                "blockedBy" => TaskLinkKind::BlockedBy,
                "relatedTo" => TaskLinkKind::RelatedTo,
                _ => return Err(invalid()),
            };
            Ok(TaskLink { from, kind, to })
        })
        .collect()
}

async fn read_mark(
    connection: &mut TursoConnection,
    project: &ProjectId,
    reader: &Identity,
    scope: &ReadScope,
) -> Result<Option<message_board::ReadMark>, ProjectStorageError> {
    let reader = serde_json::to_string(reader)?;
    let scope = serde_json::to_string(scope)?;
    let id = sqlx_turso::query_scalar!(r#"SELECT record_id AS "record_id!: String" FROM read_marks WHERE reader = ? AND scope = ?"#, reader, scope).fetch_optional(&mut *connection).await?;
    let Some(id) = id else {
        return Ok(None);
    };
    let reference = RecordRef {
        kind: RecordKind::ReadMark,
        id: RecordId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed)?,
    };
    let record = read_record(connection, &reference)
        .await?
        .ok_or(ProjectStorageError::InvalidFeed)?;
    validate_project_record(&record, project)?;
    let Record::ReadMark(mark) = record else {
        return Err(ProjectStorageError::InvalidFeed);
    };
    let high_water = crate::project_store::checkpoint(connection).await?;
    if mark.through.epoch == high_water.epoch && mark.through.position > high_water.position {
        return Err(ProjectStorageError::InvalidRecord {
            record: reference,
            field: "readMark.through",
        });
    }
    Ok(Some(mark))
}

impl ProjectSnapshot {
    pub async fn records(&mut self, kind: RecordKind) -> Result<Vec<Record>, ProjectStorageError> {
        records_of_kind(&mut self.transaction, &self.project_id, kind).await
    }
    pub async fn task_links(&mut self) -> Result<Vec<TaskLink>, ProjectStorageError> {
        read_task_links(&mut self.transaction, &self.project_id).await
    }
    pub async fn read_mark(
        &mut self,
        reader: &Identity,
        scope: &ReadScope,
    ) -> Result<Option<message_board::ReadMark>, ProjectStorageError> {
        read_mark(&mut self.transaction, &self.project_id, reader, scope).await
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkMutation {
    Add,
    Remove,
}

impl ProjectWriteTransaction {
    pub async fn records(&mut self, kind: RecordKind) -> Result<Vec<Record>, ProjectStorageError> {
        records_of_kind(&mut self.transaction, &self.project_id, kind).await
    }
    pub async fn task_links(&mut self) -> Result<Vec<TaskLink>, ProjectStorageError> {
        read_task_links(&mut self.transaction, &self.project_id).await
    }
    pub async fn read_mark(
        &mut self,
        reader: &Identity,
        scope: &ReadScope,
    ) -> Result<Option<message_board::ReadMark>, ProjectStorageError> {
        read_mark(&mut self.transaction, &self.project_id, reader, scope).await
    }
    pub async fn edit_task_link(
        &mut self,
        link: &TaskLink,
        mutation: LinkMutation,
    ) -> Result<LinkChange, ProjectStorageError> {
        let from = link.from.as_str();
        let to = link.to.as_str();
        let kind = match link.kind {
            TaskLinkKind::BlockedBy => "blockedBy",
            TaskLinkKind::RelatedTo => "relatedTo",
        };
        let result = match mutation {
            LinkMutation::Add => sqlx_turso::query!("INSERT INTO task_links(from_task, link_kind, to_task) VALUES (?, ?, ?) ON CONFLICT(from_task, link_kind, to_task) DO NOTHING", from, kind, to).execute(&mut *self.transaction).await?,
            LinkMutation::Remove => sqlx_turso::query!("DELETE FROM task_links WHERE from_task = ? AND link_kind = ? AND to_task = ?", from, kind, to).execute(&mut *self.transaction).await?,
        };
        Ok(match (mutation, result.rows_affected() == 0) {
            (LinkMutation::Add, false) => LinkChange::Added,
            (LinkMutation::Add, true) => LinkChange::AlreadyPresent,
            (LinkMutation::Remove, false) => LinkChange::Removed,
            (LinkMutation::Remove, true) => LinkChange::NotPresent,
        })
    }
}
