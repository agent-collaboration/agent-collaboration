//! Snapshot-consistent typed collections and independent task-link rows.
use crate::project_record_codec::{stored_text, task_link_kind_tag};
use crate::{ImportRowKind, ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{
    LinkChange, ProjectId, Record, RecordId, RecordKind, RecordRef, TaskId, TaskLink,
};
use sqlx::Executor;
use sqlx_turso::Turso;

async fn records_of_kind<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    kind: RecordKind,
) -> Result<Vec<Record>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let mut after = String::new();
    let mut records = Vec::new();
    loop {
        let keys = crate::project_record_keys::record_keys(transaction, kind, &after, 100).await?;
        let Some(last) = keys.last() else { break };
        after = last.id.as_str().to_owned();
        let rows = crate::project_record_batch::record_batch(transaction, project, &keys).await?;
        if rows.len() != keys.len() {
            return Err(ProjectStorageError::InvalidFeed);
        }
        for row in rows {
            records.push(row.decode()?);
        }
    }
    Ok(records)
}

async fn read_task_links<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
) -> Result<Vec<TaskLink>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let rows = sqlx_turso::query!(r#"SELECT l.link_id AS "link_id!: String", l.from_task AS "from_task!: String", l.link_kind AS "link_kind!: String", l.to_task AS "to_task!: String", fw.project_id AS "from_project?: String", tw.project_id AS "to_project?: String" FROM task_links l LEFT JOIN tasks ft ON ft.task_id=l.from_task LEFT JOIN workstreams fw ON fw.workstream_id=ft.workstream_id LEFT JOIN tasks tt ON tt.task_id=l.to_task LEFT JOIN workstreams tw ON tw.workstream_id=tt.workstream_id ORDER BY l.from_task, l.link_kind, l.to_task"#)
        .fetch_all(&mut *transaction).await?;
    rows.into_iter()
        .map(|row| {
            let reference: RecordRef = project.clone().into();
            let invalid = || ProjectStorageError::InvalidRecord {
                record: reference.clone(),
                field: "taskLink",
            };
            RecordId::try_from(row.link_id).map_err(|_| invalid())?;
            if row.from_project.as_deref() != Some(project.as_str())
                || row.to_project.as_deref() != Some(project.as_str())
            {
                return Err(invalid());
            }
            Ok(TaskLink {
                from: stored_text(row.from_task, &reference, "from", TaskId::as_str)?,
                to: stored_text(row.to_task, &reference, "to", TaskId::as_str)?,
                kind: crate::project_record_codec::stored_tag(
                    row.link_kind,
                    &reference,
                    "linkKind",
                )?,
            })
        })
        .collect()
}

impl ProjectSnapshot {
    pub async fn records(&mut self, kind: RecordKind) -> Result<Vec<Record>, ProjectStorageError> {
        records_of_kind(&mut self.transaction, &self.project_id, kind).await
    }
    pub async fn task_links(&mut self) -> Result<Vec<TaskLink>, ProjectStorageError> {
        read_task_links(&mut self.transaction, &self.project_id).await
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
    pub async fn edit_task_link(
        &mut self,
        link: &TaskLink,
        mutation: LinkMutation,
    ) -> Result<LinkChange, ProjectStorageError> {
        let from = link.from.as_str();
        let to = link.to.as_str();
        let kind = task_link_kind_tag(link.kind);
        let existing = sqlx_turso::query_scalar!(r#"SELECT link_id AS "link_id!: String" FROM task_links WHERE from_task=? AND link_kind=? AND to_task=?"#, from, kind, to).fetch_optional(&mut self.transaction).await?;
        let existing = existing.map(RecordId::try_from).transpose().map_err(|_| {
            ProjectStorageError::InvalidRecord {
                record: link.from.clone().into(),
                field: "taskLink.id",
            }
        })?;
        match (mutation, existing) {
            (LinkMutation::Add, Some(_)) => Ok(LinkChange::AlreadyPresent),
            (LinkMutation::Remove, None) => Ok(LinkChange::NotPresent),
            (LinkMutation::Add, None) => {
                let id = self
                    .admission
                    .row_key(ImportRowKind::TaskLink, &[from, kind, to])?;
                let id = id.as_str();
                sqlx_turso::query!("INSERT INTO task_links(link_id, from_task, link_kind, to_task) VALUES (?, ?, ?, ?)", id, from, kind, to).execute(&mut self.transaction).await?;
                Ok(LinkChange::Added)
            }
            (LinkMutation::Remove, Some(id)) => {
                let id = id.as_str();
                let changed = sqlx_turso::query!("DELETE FROM task_links WHERE link_id=?", id)
                    .execute(&mut self.transaction)
                    .await?;
                if changed.rows_affected() != 1 {
                    return Err(ProjectStorageError::InvalidFeed);
                }
                Ok(LinkChange::Removed)
            }
        }
    }
}
