use crate::project_record_codec::{
    ProjectRowEnvelope, ProjectRowWrite, require_one_row, task_link_kind_tag, timestamp,
};
use crate::{ProjectStorageError, ProjectWriteTransaction};
use message_board::{
    Fact, PlanRevision, PlanRevisionHash, RecordKind, RecordRef, TaskLinkEvent, TaskLinkEventKind,
};

impl ProjectWriteTransaction {
    pub(crate) async fn write_fact(
        &mut self,
        row: &Fact,
        envelope: &ProjectRowEnvelope,
    ) -> Result<(), ProjectStorageError> {
        let id = row.fact_id.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let fact = serde_json::to_string(&row.kind)?;
        let links = serde_json::to_string(&row.links)?;
        let producer_time = timestamp(row.producer_time);
        let anchor_epoch = row
            .anchor
            .map(|cursor| i64::try_from(cursor.epoch.get()))
            .transpose()
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let anchor_position = row
            .anchor
            .map(|cursor| i64::try_from(cursor.position.get()))
            .transpose()
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let changed = sqlx_turso::query!(
            "INSERT INTO facts(fact_id, actor_id, request_id, epoch, position, router_time, time_basis, fact, links, producer_time, anchor_epoch, anchor_position, late) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            id, actor, request, envelope.epoch, envelope.position, envelope.router_time,
            envelope.time_basis, fact, links, producer_time, anchor_epoch, anchor_position, row.late
        ).execute(&mut self.transaction).await?;
        require_one_row(changed.rows_affected(), row.fact_id.clone().into())
    }

    pub(crate) async fn write_plan_revision(
        &mut self,
        row: &PlanRevision,
        envelope: &ProjectRowEnvelope,
        mode: ProjectRowWrite,
    ) -> Result<(), ProjectStorageError> {
        let id = row.envelope.id.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let hash = row.hash.as_str();
        let document = serde_json::to_string(&row.document)?;
        let superseded_by = row.superseded_by.as_ref().map(PlanRevisionHash::as_str);
        let revision =
            i64::try_from(row.revision.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let changed = match mode {
            ProjectRowWrite::Insert => sqlx_turso::query!(
                "INSERT INTO plan_revisions(revision_id, actor_id, request_id, epoch, position, router_time, time_basis, content_hash, document, superseded_by, revision) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                id, actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, hash, document, superseded_by, revision
            ).execute(&mut self.transaction).await?,
            ProjectRowWrite::Update => sqlx_turso::query!(
                "UPDATE plan_revisions SET actor_id=?, request_id=?, epoch=?, position=?, router_time=?, time_basis=?, content_hash=?, document=?, superseded_by=?, revision=? WHERE revision_id=?",
                actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, hash, document, superseded_by, revision, id
            ).execute(&mut self.transaction).await?,
        };
        require_one_row(
            changed.rows_affected(),
            RecordRef {
                kind: RecordKind::PlanRevision,
                id: row.envelope.id.clone(),
            },
        )
    }

    pub(crate) async fn write_task_link_event(
        &mut self,
        row: &TaskLinkEvent,
        envelope: &ProjectRowEnvelope,
    ) -> Result<(), ProjectStorageError> {
        let (change_kind, link) = match &row.change {
            TaskLinkEventKind::Added(link) => ("added", link),
            TaskLinkEventKind::Removed(link) => ("removed", link),
        };
        let id = row.id.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let from = link.from.as_str();
        let to = link.to.as_str();
        let kind = task_link_kind_tag(link.kind);
        let changed = sqlx_turso::query!(
            "INSERT INTO task_link_events(event_id, actor_id, request_id, epoch, position, router_time, time_basis, change_kind, from_task, link_kind, to_task) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            id, actor, request, envelope.epoch, envelope.position, envelope.router_time,
            envelope.time_basis, change_kind, from, kind, to
        ).execute(&mut self.transaction).await?;
        require_one_row(changed.rows_affected(), row.id.clone().into())
    }
}
