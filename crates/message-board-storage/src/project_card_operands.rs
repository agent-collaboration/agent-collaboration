//! A non-lead subject card needs its tasks and their direct blockers, not sibling bodies.
use crate::{ProjectSnapshot, ProjectStorageError};
use message_board::*;
use std::collections::BTreeSet;
pub struct CardWorkOperands {
    pub workstream: Option<WorkstreamId>,
    pub records: Vec<RecordRef>,
    pub links: Vec<TaskLink>,
}
impl ProjectSnapshot {
    pub async fn card_work_operands(
        &mut self,
        subject: &TrailSubject,
    ) -> Result<CardWorkOperands, ProjectStorageError> {
        let mut records = Vec::new();
        let mut task_ids = BTreeSet::new();
        let workstream = match subject {
            TrailSubject::Task(id) => {
                let id = id.as_str();
                let row = sqlx_turso::query_scalar!(
                    r#"SELECT workstream_id AS "workstream!: String" FROM tasks WHERE task_id = ?"#,
                    id
                )
                .fetch_optional(&mut *self.transaction)
                .await?;
                if let Some(workstream) = row {
                    task_ids.insert(
                        TaskId::try_from(id.to_owned())
                            .map_err(|_| ProjectStorageError::InvalidFeed)?,
                    );
                    Some(
                        WorkstreamId::try_from(workstream)
                            .map_err(|_| ProjectStorageError::InvalidFeed)?,
                    )
                } else {
                    None
                }
            }
            TrailSubject::Workstream(id) => {
                let scope = id.as_str();
                let exists = sqlx_turso::query_scalar!(r#"SELECT workstream_id AS "workstream!: String" FROM workstreams WHERE workstream_id = ?"#, scope).fetch_optional(&mut *self.transaction).await?;
                if exists.is_some() {
                    let mut after = String::new();
                    loop {
                        let rows = sqlx_turso::query_scalar!(r#"SELECT task_id AS "task!: String" FROM tasks WHERE workstream_id = ? AND task_id > ? ORDER BY task_id LIMIT 101"#, scope, after).fetch_all(&mut *self.transaction).await?;
                        let exhausted = rows.len() <= 100;
                        for id in rows.into_iter().take(100) {
                            after = id.clone();
                            task_ids.insert(
                                TaskId::try_from(id)
                                    .map_err(|_| ProjectStorageError::InvalidFeed)?,
                            );
                        }
                        if exhausted {
                            break;
                        }
                    }
                    Some(id.clone())
                } else {
                    None
                }
            }
            TrailSubject::Project => return Err(ProjectStorageError::InvalidFeed),
        };
        let keys = serde_json::to_string(&task_ids)?;
        let rows = sqlx_turso::query!(r#"SELECT l.from_task AS "from_task!: String", l.link_kind AS "kind!: String", l.to_task AS "to_task!: String" FROM json_each(?) k CROSS JOIN task_links l ON l.from_task = CAST(k.value AS TEXT) ORDER BY l.from_task, l.link_kind, l.to_task"#, keys).fetch_all(&mut *self.transaction).await?;
        let links = rows
            .into_iter()
            .map(|row| {
                Ok(TaskLink {
                    from: TaskId::try_from(row.from_task)
                        .map_err(|_| ProjectStorageError::InvalidFeed)?,
                    to: TaskId::try_from(row.to_task)
                        .map_err(|_| ProjectStorageError::InvalidFeed)?,
                    kind: serde_json::from_value(serde_json::Value::String(row.kind))
                        .map_err(|_| ProjectStorageError::InvalidFeed)?,
                })
            })
            .collect::<Result<Vec<_>, ProjectStorageError>>()?;
        for link in &links {
            if link.kind == TaskLinkKind::BlockedBy {
                task_ids.insert(link.to.clone());
            }
        }
        records.extend(task_ids.into_iter().map(RecordRef::from));
        if let Some(id) = &workstream {
            records.push(id.clone().into());
        }
        Ok(CardWorkOperands {
            workstream,
            records,
            links,
        })
    }
    pub async fn active_actor_subject_role_keys(
        &mut self,
        actor: &Identity,
        subject: &RoleSubject,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        crate::project_role_operands::active_actor_subject_role_keys(
            &mut self.transaction,
            actor,
            subject,
        )
        .await
    }
}
