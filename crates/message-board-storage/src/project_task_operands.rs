//! Task writes select endpoint bodies and only the graph edges their rule traverses.
use crate::{ProjectStorageError, ProjectWriteTransaction};
use message_board::*;

fn task_link(
    from: String,
    kind: String,
    to: String,
    project: &ProjectId,
) -> Result<TaskLink, ProjectStorageError> {
    let invalid = || ProjectStorageError::InvalidRecord {
        record: project.clone().into(),
        field: "taskLink",
    };
    Ok(TaskLink {
        from: TaskId::try_from(from).map_err(|_| invalid())?,
        kind: match kind.as_str() {
            "blockedBy" => TaskLinkKind::BlockedBy,
            "relatedTo" => TaskLinkKind::RelatedTo,
            _ => return Err(invalid()),
        },
        to: TaskId::try_from(to).map_err(|_| invalid())?,
    })
}
impl ProjectWriteTransaction {
    pub async fn task_link_operand(
        &mut self,
        link: &TaskLink,
    ) -> Result<Option<TaskLink>, ProjectStorageError> {
        let from = link.from.as_str();
        let to = link.to.as_str();
        let kind = match link.kind {
            TaskLinkKind::BlockedBy => "blockedBy",
            TaskLinkKind::RelatedTo => "relatedTo",
        };
        let row = sqlx_turso::query!(r#"SELECT from_task AS "from_task!: String", link_kind AS "link_kind!: String", to_task AS "to_task!: String" FROM task_links WHERE from_task = ? AND link_kind = ? AND to_task = ?"#, from, kind, to).fetch_optional(&mut self.transaction).await?;
        row.map(|row| task_link(row.from_task, row.link_kind, row.to_task, &self.project_id))
            .transpose()
    }
    pub async fn live_subtask_operand(
        &mut self,
        subtask: &SubtaskId,
    ) -> Result<bool, ProjectStorageError> {
        let id = subtask.as_str();
        let exists = sqlx_turso::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM subtasks WHERE subtask_id = ? AND removed=0) AS "exists!: i64""#,
            id
        )
        .fetch_one(&mut self.transaction)
        .await?;
        match exists {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ProjectStorageError::InvalidFeed),
        }
    }
}
