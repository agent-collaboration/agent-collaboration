use crate::project_record_codec::{ProjectRowEnvelope, ProjectRowWrite, require_one_row};
use crate::{ImportRowKind, ProjectStorageError, ProjectWriteTransaction};
use message_board::*;

impl ProjectWriteTransaction {
    pub(crate) async fn write_thread(
        &mut self,
        row: &Thread,
        envelope: &ProjectRowEnvelope,
        mode: ProjectRowWrite,
    ) -> Result<(), ProjectStorageError> {
        let id = row.thread_id.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let about = serde_json::to_string(&row.about)?;
        let state = serde_json::to_string(&row.state)?;
        // Current holders are derived from participation rows, never stored again here.
        let changed = match mode {
            ProjectRowWrite::Insert => sqlx_turso::query!(
                "INSERT INTO threads(thread_id, actor_id, request_id, epoch, position, router_time, time_basis, about, imported, state) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                id, actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, about, row.imported, state
            ).execute(&mut self.transaction).await?,
            ProjectRowWrite::Update => sqlx_turso::query!(
                "UPDATE threads SET actor_id=?, request_id=?, epoch=?, position=?, router_time=?, time_basis=?, about=?, imported=?, state=? WHERE thread_id=?",
                actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, about, row.imported, state, id
            ).execute(&mut self.transaction).await?,
        };
        require_one_row(changed.rows_affected(), row.thread_id.clone().into())
    }

    pub(crate) async fn write_post(
        &mut self,
        row: &Post,
        envelope: &ProjectRowEnvelope,
    ) -> Result<(), ProjectStorageError> {
        if row.envelope.actor.declared_identity().as_ref() != Some(&row.actor) {
            return Err(ProjectStorageError::InvalidRecord {
                record: row.post_id.clone().into(),
                field: "actor",
            });
        }
        let id = row.post_id.as_str();
        let (thread, placement_kind) = match &row.placement {
            Placement::Project { .. } => (ThreadId::from(row.post_id.clone()), "project"),
            Placement::Thread { thread_id } => (thread_id.clone(), "thread"),
        };
        let thread = thread.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let acting_for = row
            .acting_for
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let text = row.text.as_str();
        let posted_as_role = row
            .posted_as_role
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let changed = sqlx_turso::query!(
            "INSERT INTO posts(post_id, thread_id, actor_id, request_id, epoch, position, router_time, time_basis, placement_kind, acting_for, text, posted_as_role) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            id, thread, actor, request, envelope.epoch, envelope.position, envelope.router_time,
            envelope.time_basis, placement_kind, acting_for, text, posted_as_role
        ).execute(&mut self.transaction).await?;
        require_one_row(changed.rows_affected(), row.post_id.clone().into())?;
        for (place, target) in row.references.iter().enumerate() {
            let place = i64::try_from(place).map_err(|_| ProjectStorageError::InvalidFeed)?;
            let place_text = place.to_string();
            let reference_id = self
                .admission
                .row_key(ImportRowKind::PostReference, &[id, &place_text])?;
            let reference_id = reference_id.as_str();
            let target = serde_json::to_string(target)?;
            sqlx_turso::query!(
                "INSERT INTO post_references(reference_id, source_post, place, target) VALUES (?, ?, ?, ?)",
                reference_id, id, place, target
            ).execute(&mut self.transaction).await?;
        }
        Ok(())
    }

    pub(crate) async fn write_thread_refiling(
        &mut self,
        row: &ThreadRefiled,
        envelope: &ProjectRowEnvelope,
    ) -> Result<(), ProjectStorageError> {
        let id = row.envelope.id.as_str();
        let thread = row.thread.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let from = serde_json::to_string(&row.from)?;
        let to = serde_json::to_string(&row.to)?;
        let changed = sqlx_turso::query!(
            "INSERT INTO thread_refilings(refiling_id, thread_id, actor_id, request_id, epoch, position, router_time, time_basis, from_subject, to_subject) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            id, thread, actor, request, envelope.epoch, envelope.position, envelope.router_time,
            envelope.time_basis, from, to
        ).execute(&mut self.transaction).await?;
        require_one_row(
            changed.rows_affected(),
            RecordRef {
                kind: RecordKind::ThreadRefiled,
                id: row.envelope.id.clone(),
            },
        )
    }
}
