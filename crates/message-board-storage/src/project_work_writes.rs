use crate::project_record_codec::{ProjectRowEnvelope, ProjectRowWrite, require_one_row};
use crate::{ProjectStorageError, ProjectWriteTransaction};
use message_board::{ProjectState, Subtask, Task, Workstream};

impl ProjectWriteTransaction {
    pub(crate) async fn write_project_state(
        &mut self,
        row: &ProjectState,
        envelope: &ProjectRowEnvelope,
        mode: ProjectRowWrite,
    ) -> Result<(), ProjectStorageError> {
        let id = row.project_id.as_str();
        let workspace = row.envelope.scope.workspace.as_str();
        let board = row
            .envelope
            .scope
            .board
            .as_ref()
            .ok_or_else(|| ProjectStorageError::InvalidRecord {
                record: row.project_id.clone().into(),
                field: "board",
            })?
            .as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let description = row.description.as_str();
        let revision =
            i64::try_from(row.revision.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let closed_epoch = row
            .closed_at
            .map(|cursor| i64::try_from(cursor.epoch.get()))
            .transpose()
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let closed_position = row
            .closed_at
            .map(|cursor| i64::try_from(cursor.position.get()))
            .transpose()
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let changed = match mode {
            ProjectRowWrite::Insert => sqlx_turso::query!(
                "INSERT INTO project_state(project_id, workspace_id, board_id, actor_id, request_id, epoch, position, router_time, time_basis, description, revision, closed_epoch, closed_position) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                id, workspace, board, actor, request, envelope.epoch, envelope.position,
                envelope.router_time, envelope.time_basis, description, revision, closed_epoch, closed_position
            ).execute(&mut self.transaction).await?,
            ProjectRowWrite::Update => sqlx_turso::query!(
                "UPDATE project_state SET actor_id=?, request_id=?, epoch=?, position=?, router_time=?, time_basis=?, description=?, revision=?, closed_epoch=?, closed_position=? WHERE project_id=?",
                actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, description, revision, closed_epoch, closed_position, id
            ).execute(&mut self.transaction).await?,
        };
        require_one_row(changed.rows_affected(), row.project_id.clone().into())
    }

    pub(crate) async fn write_workstream(
        &mut self,
        row: &Workstream,
        envelope: &ProjectRowEnvelope,
        mode: ProjectRowWrite,
    ) -> Result<(), ProjectStorageError> {
        let id = row.workstream_id.as_str();
        let project = row.project_id.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let name = row.name.as_str();
        let state = serde_json::to_string(&row.state)?;
        let revision =
            i64::try_from(row.revision.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let changed = match mode {
            ProjectRowWrite::Insert => sqlx_turso::query!(
                "INSERT INTO workstreams(workstream_id, project_id, actor_id, request_id, epoch, position, router_time, time_basis, name, goal, state, revision) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                id, project, actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, name, row.goal, state, revision
            ).execute(&mut self.transaction).await?,
            ProjectRowWrite::Update => sqlx_turso::query!(
                "UPDATE workstreams SET actor_id=?, request_id=?, epoch=?, position=?, router_time=?, time_basis=?, name=?, goal=?, state=?, revision=? WHERE workstream_id=?",
                actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, name, row.goal, state, revision, id
            ).execute(&mut self.transaction).await?,
        };
        require_one_row(changed.rows_affected(), row.workstream_id.clone().into())
    }

    pub(crate) async fn write_task(
        &mut self,
        row: &Task,
        envelope: &ProjectRowEnvelope,
        mode: ProjectRowWrite,
    ) -> Result<(), ProjectStorageError> {
        let id = row.task_id.as_str();
        let workstream = row.workstream_id.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let title = row.title.as_str();
        let status = serde_json::to_string(&row.status)?;
        let revision =
            i64::try_from(row.revision.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let changed = match mode {
            ProjectRowWrite::Insert => sqlx_turso::query!(
                "INSERT INTO tasks(task_id, workstream_id, actor_id, request_id, epoch, position, router_time, time_basis, title, body, status, revision) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                id, workstream, actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, title, row.body, status, revision
            ).execute(&mut self.transaction).await?,
            ProjectRowWrite::Update => sqlx_turso::query!(
                "UPDATE tasks SET workstream_id=?, actor_id=?, request_id=?, epoch=?, position=?, router_time=?, time_basis=?, title=?, body=?, status=?, revision=? WHERE task_id=?",
                workstream, actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, title, row.body, status, revision, id
            ).execute(&mut self.transaction).await?,
        };
        require_one_row(changed.rows_affected(), row.task_id.clone().into())
    }

    pub(crate) async fn write_subtask(
        &mut self,
        row: &Subtask,
        envelope: &ProjectRowEnvelope,
        mode: ProjectRowWrite,
    ) -> Result<(), ProjectStorageError> {
        let id = row.subtask_id.as_str();
        let task = row.task_id.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let text = row.text.as_str();
        let place = String::from(row.place.clone());
        let changed = match mode {
            ProjectRowWrite::Insert => sqlx_turso::query!(
                "INSERT INTO subtasks(subtask_id, task_id, actor_id, request_id, epoch, position, router_time, time_basis, text, checked, removed, place) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                id, task, actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, text, row.checked, row.removed, place
            ).execute(&mut self.transaction).await?,
            ProjectRowWrite::Update => sqlx_turso::query!(
                "UPDATE subtasks SET actor_id=?, request_id=?, epoch=?, position=?, router_time=?, time_basis=?, text=?, checked=?, removed=?, place=? WHERE subtask_id=?",
                actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, text, row.checked, row.removed, place, id
            ).execute(&mut self.transaction).await?,
        };
        require_one_row(changed.rows_affected(), row.subtask_id.clone().into())
    }
}
