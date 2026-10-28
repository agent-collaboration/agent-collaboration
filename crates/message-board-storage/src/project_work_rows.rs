//! Typed work rows stay undecoded until their page consumer needs them.
use crate::ProjectStorageError;
use crate::project_record_codec::{
    stored_bool, stored_counter, stored_json, stored_project_row, stored_text,
};
use message_board::{
    Description, FeedCursor, ProjectId, ProjectState, Record, RecordKind, RecordRef, ResourceName,
    Subtask, SubtaskId, SubtaskPlace, Task, TaskId, TaskLink, TaskLinkEvent, TaskLinkEventId,
    TaskLinkEventKind, TaskLinkKind, TaskTitle, TrailText, Workstream, WorkstreamId,
};
use sqlx::Executor;
use sqlx_turso::Turso;

stored_project_row!(StoredProjectState, ProjectStateBody {
    description: String, revision: i64,
    closed_epoch: Option<i64>, closed_position: Option<i64>,
});
stored_project_row!(
    StoredWorkstream,
    WorkstreamBody {
        name: String,
        goal: String,
        state: String,
        revision: i64,
    }
);
stored_project_row!(
    StoredTask,
    TaskBody {
        workstream_id: String,
        title: String,
        body: String,
        status: String,
        revision: i64,
    }
);
stored_project_row!(
    StoredSubtask,
    SubtaskBody {
        task_id: String,
        text: String,
        checked: i64,
        removed: i64,
        place: String,
    }
);
stored_project_row!(
    StoredTaskLinkEvent,
    TaskLinkEventBody {
        change_kind: String,
        from_task: String,
        link_kind: String,
        to_task: String,
        target_project: String,
    }
);

pub(crate) enum StoredWorkRecord {
    Project(StoredProjectState),
    Workstream(StoredWorkstream),
    Task(StoredTask),
    Subtask(StoredSubtask),
    TaskLinkEvent(StoredTaskLinkEvent),
}

impl StoredWorkRecord {
    pub(crate) fn reference(&self) -> Result<RecordRef, ProjectStorageError> {
        match self {
            Self::Project(row) => row.reference(RecordKind::Project),
            Self::Workstream(row) => row.reference(RecordKind::Workstream),
            Self::Task(row) => row.reference(RecordKind::Task),
            Self::Subtask(row) => row.reference(RecordKind::Subtask),
            Self::TaskLinkEvent(row) => row.reference(RecordKind::TaskLinkEvent),
        }
    }

    pub(crate) fn decode(self, project: &ProjectId) -> Result<Record, ProjectStorageError> {
        let reference = self.reference()?;
        let invalid = |field| ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        };
        Ok(match self {
            Self::Project(row) => {
                let (raw, body) = row.into_parts();
                let envelope = raw.decode(&reference, project)?;
                let closed_at = match (body.closed_epoch, body.closed_position) {
                    (None, None) => None,
                    (Some(epoch), Some(position)) => Some(FeedCursor {
                        epoch: stored_counter(epoch, &reference, "closedAt")?,
                        position: stored_counter(position, &reference, "closedAt")?,
                    }),
                    _ => return Err(invalid("closedAt")),
                };
                Record::Project(ProjectState {
                    envelope,
                    project_id: project.clone(),
                    description: stored_text(
                        body.description,
                        &reference,
                        "description",
                        Description::as_str,
                    )?,
                    revision: stored_counter(body.revision, &reference, "revision")?,
                    closed_at,
                })
            }
            Self::Workstream(row) => {
                let (raw, body) = row.into_parts();
                Record::Workstream(Workstream {
                    envelope: raw.decode(&reference, project)?,
                    workstream_id: WorkstreamId::try_from(reference.id.as_str().to_owned())
                        .map_err(|_| invalid("id"))?,
                    project_id: project.clone(),
                    name: stored_text(body.name, &reference, "name", ResourceName::as_str)?,
                    goal: body.goal,
                    state: stored_json(&body.state, &reference, "state")?,
                    revision: stored_counter(body.revision, &reference, "revision")?,
                })
            }
            Self::Task(row) => {
                let (raw, body) = row.into_parts();
                Record::Task(Task {
                    envelope: raw.decode(&reference, project)?,
                    task_id: TaskId::try_from(reference.id.as_str().to_owned())
                        .map_err(|_| invalid("id"))?,
                    project_id: project.clone(),
                    workstream_id: stored_text(
                        body.workstream_id,
                        &reference,
                        "workstreamId",
                        WorkstreamId::as_str,
                    )?,
                    title: stored_text(body.title, &reference, "title", TaskTitle::as_str)?,
                    body: body.body,
                    status: stored_json(&body.status, &reference, "status")?,
                    revision: stored_counter(body.revision, &reference, "revision")?,
                })
            }
            Self::Subtask(row) => {
                let (raw, body) = row.into_parts();
                Record::Subtask(Subtask {
                    envelope: raw.decode(&reference, project)?,
                    subtask_id: SubtaskId::try_from(reference.id.as_str().to_owned())
                        .map_err(|_| invalid("id"))?,
                    task_id: stored_text(body.task_id, &reference, "taskId", TaskId::as_str)?,
                    text: stored_text(body.text, &reference, "text", TrailText::as_str)?,
                    checked: stored_bool(body.checked, &reference, "checked")?,
                    removed: stored_bool(body.removed, &reference, "removed")?,
                    place: SubtaskPlace::try_from(body.place).map_err(|_| invalid("place"))?,
                })
            }
            Self::TaskLinkEvent(row) => {
                let (raw, body) = row.into_parts();
                if body.target_project != project.as_str() {
                    return Err(invalid("project"));
                }
                let link = TaskLink {
                    from: stored_text(body.from_task, &reference, "from", TaskId::as_str)?,
                    to: stored_text(body.to_task, &reference, "to", TaskId::as_str)?,
                    kind: match body.link_kind.as_str() {
                        "blockedBy" => TaskLinkKind::BlockedBy,
                        "relatedTo" => TaskLinkKind::RelatedTo,
                        _ => return Err(invalid("linkKind")),
                    },
                };
                let change = match body.change_kind.as_str() {
                    "added" => TaskLinkEventKind::Added(link),
                    "removed" => TaskLinkEventKind::Removed(link),
                    _ => return Err(invalid("changeKind")),
                };
                Record::TaskLinkEvent(TaskLinkEvent {
                    envelope: raw.decode(&reference, project)?,
                    id: TaskLinkEventId::try_from(reference.id.as_str().to_owned())
                        .map_err(|_| invalid("id"))?,
                    change,
                })
            }
        })
    }
}

pub(crate) async fn work_records<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    kind: RecordKind,
    keys: &[&str],
) -> Result<Vec<StoredWorkRecord>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let project = project.as_str();
    let keys = serde_json::to_string(keys)?;
    Ok(match kind {
        RecordKind::Project => sqlx_turso::query_file_as!(
            StoredProjectState,
            "src/project_state_batch.sql",
            project,
            keys
        )
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .map(StoredWorkRecord::Project)
        .collect(),
        RecordKind::Workstream => sqlx_turso::query_file_as!(
            StoredWorkstream,
            "src/project_workstream_batch.sql",
            project,
            keys
        )
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .map(StoredWorkRecord::Workstream)
        .collect(),
        RecordKind::Task => {
            sqlx_turso::query_file_as!(StoredTask, "src/project_task_batch.sql", project, keys)
                .fetch_all(&mut *transaction)
                .await?
                .into_iter()
                .map(StoredWorkRecord::Task)
                .collect()
        }
        RecordKind::Subtask => sqlx_turso::query_file_as!(
            StoredSubtask,
            "src/project_subtask_batch.sql",
            project,
            keys
        )
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .map(StoredWorkRecord::Subtask)
        .collect(),
        RecordKind::TaskLinkEvent => sqlx_turso::query_file_as!(
            StoredTaskLinkEvent,
            "src/project_task_link_event_batch.sql",
            project,
            keys
        )
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .map(StoredWorkRecord::TaskLinkEvent)
        .collect(),
        _ => return Err(ProjectStorageError::InvalidFeed),
    })
}
