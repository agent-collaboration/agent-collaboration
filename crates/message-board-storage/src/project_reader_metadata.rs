//! Reader-state counts and bounded references use existing feed and subject metadata.
use crate::{ProjectSnapshot, ProjectStorageError, project_record_io::record_kind_tag};
use message_board::*;

struct UnreadMetadataRow {
    kind: String,
    id: String,
    count: i64,
}

impl ProjectSnapshot {
    pub async fn subtask_task_metadata(
        &mut self,
        subtask: &SubtaskId,
    ) -> Result<Option<TaskId>, ProjectStorageError> {
        let id = subtask.as_str();
        let row = sqlx_turso::query!(r#"SELECT s.subtask_id AS "id!: String", s.task_id AS "task!: String", w.project_id AS "project?: String" FROM subtasks s LEFT JOIN tasks t ON t.task_id=s.task_id LEFT JOIN workstreams w ON w.workstream_id=t.workstream_id WHERE s.subtask_id=?"#, id).fetch_optional(&mut self.transaction).await?;
        row.map(|row| {
            let reference = subtask.clone().into();
            if row.id != id || row.project.as_deref() != Some(self.project_id.as_str()) {
                return Err(ProjectStorageError::InvalidRecord {
                    record: reference,
                    field: "scope",
                });
            }
            crate::project_record_codec::stored_text(row.task, &reference, "taskId", TaskId::as_str)
        })
        .transpose()
    }
    pub async fn trail_subject_metadata(
        &mut self,
        trail: &WorkTrailRowId,
    ) -> Result<Option<TrailSubject>, ProjectStorageError> {
        let id = trail.as_str();
        let row = sqlx_turso::query!(r#"SELECT row_id AS "id!: String", subject AS "subject!: String" FROM work_trail_rows WHERE row_id=?"#, id).fetch_optional(&mut self.transaction).await?;
        row.map(|row| {
            if row.id != id {
                return Err(ProjectStorageError::InvalidFeed);
            }
            crate::project_record_codec::stored_json(&row.subject, &trail.clone().into(), "subject")
        })
        .transpose()
    }

    pub async fn unread_state_after(
        &mut self,
        subject: &RecordRef,
        after: FeedCursor,
    ) -> Result<ReaderState, ProjectStorageError> {
        let high = self.high_water();
        let after = match self.cursor_validity(after).await? {
            crate::CursorValidity::Valid => after,
            crate::CursorValidity::Superseded { resume_from } => resume_from,
            crate::CursorValidity::Invalid => return Err(ProjectStorageError::InvalidCursor),
        };
        self.validate_feed_range(after.position, high.position)
            .await?;
        let start =
            i64::try_from(after.position.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let end =
            i64::try_from(high.position.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let kind = record_kind_tag(subject.kind);
        let id = subject.id.as_str();
        // Separate checked statements bound native parser depth to the requested subject.
        // No body is transferred or domain-decoded for an unread count. Retained child
        // entries carry their task even after checklist removal; task ancestry follows
        // the existing task relation. A project subject belongs only to this snapshot.
        let rows: Vec<UnreadMetadataRow> = match subject.kind {
            RecordKind::Project if id != self.project_id.as_str() => Vec::new(),
            RecordKind::Project => sqlx_turso::query!(r#"SELECT record_kind AS "record_kind!: String", record_id AS "record_id!: String", COUNT(*) OVER () AS "unread_count!: i64" FROM project_feed WHERE position > ? AND position <= ? ORDER BY position DESC LIMIT 50"#, start, end)
                .fetch_all(&mut self.transaction).await?.into_iter().map(|row| UnreadMetadataRow { kind: row.record_kind, id: row.record_id, count: row.unread_count }).collect(),
            RecordKind::Thread => sqlx_turso::query_file!("src/project_thread_unread.sql", id, start, end)
                .fetch_all(&mut self.transaction).await?.into_iter().map(|row| UnreadMetadataRow { kind: row.record_kind, id: row.record_id, count: row.unread_count }).collect(),
            RecordKind::Task => sqlx_turso::query!(r#"
WITH subject AS (SELECT 'task' AS kind, ? AS id)
SELECT record_kind AS "record_kind!: String", record_id AS "record_id!: String", COUNT(*) OVER () AS "unread_count!: i64" FROM project_feed, subject
WHERE position > ? AND position <= ? AND (
(record_kind = subject.kind AND record_id = subject.id)
OR (record_kind = 'subtask' AND json_extract(content, '$.record.record.taskId') = subject.id)
OR (record_kind = 'taskLinkEvent' AND json_extract(content, '$.record.record.change.link.from') = subject.id)
OR (record_kind = 'workTrailRow' AND json_extract(content, '$.record.record.subject.kind') = 'task' AND json_extract(content, '$.record.record.subject.subject') = subject.id)
OR (record_kind = 'fact' AND (
 EXISTS (SELECT 1 FROM json_each(json_extract(content, '$.record.record.links')) AS link WHERE json_extract(link.value, '$.kind') = subject.kind AND json_extract(link.value, '$.id') = subject.id)
 OR (json_extract(content, '$.record.record.kind.kind') = 'evaluatorResult' AND json_extract(content, '$.record.record.kind.fact.subject.kind') = subject.kind AND json_extract(content, '$.record.record.kind.fact.subject.id') = subject.id)
 OR (json_extract(content, '$.record.record.kind.kind') = 'lifecycle' AND json_extract(content, '$.record.record.kind.fact.kind') = 'dropped' AND json_extract(content, '$.record.record.kind.fact.task') = subject.id)))
OR (record_kind = 'notifyIntent' AND EXISTS (SELECT 1 FROM notify_intents AS intent WHERE intent.intent_id = project_feed.record_id AND intent.cause_kind = subject.kind AND intent.cause_id = subject.id))
OR (record_kind = 'request' AND json_extract(content, '$.record.record.decision.kind') = 'accepted' AND EXISTS (SELECT 1 FROM project_feed AS effect INDEXED BY feed_by_request_position WHERE effect.request_id = project_feed.request_id AND effect.record_kind = subject.kind AND effect.record_id = subject.id AND effect.record_kind <> 'request'))
) ORDER BY position DESC LIMIT 50
"#, id, start, end)
                .fetch_all(&mut self.transaction).await?.into_iter().map(|row| UnreadMetadataRow { kind: row.record_kind, id: row.record_id, count: row.unread_count }).collect(),
            RecordKind::Workstream => sqlx_turso::query!(r#"
WITH subject AS (SELECT 'workstream' AS kind, ? AS id)
SELECT record_kind AS "record_kind!: String", record_id AS "record_id!: String", COUNT(*) OVER () AS "unread_count!: i64" FROM project_feed, subject
WHERE position > ? AND position <= ? AND (
(record_kind = subject.kind AND record_id = subject.id)
OR (record_kind = 'task' AND json_extract(content, '$.record.record.workstreamId') = subject.id)
OR (record_kind = 'subtask' AND EXISTS (SELECT 1 FROM tasks AS task WHERE task.task_id = json_extract(content, '$.record.record.taskId') AND task.workstream_id = subject.id))
OR (record_kind = 'taskLinkEvent' AND EXISTS (SELECT 1 FROM tasks AS task WHERE task.task_id = json_extract(content, '$.record.record.change.link.from') AND task.workstream_id = subject.id))
OR (record_kind = 'workTrailRow' AND (
 (json_extract(content, '$.record.record.subject.kind') = 'workstream' AND json_extract(content, '$.record.record.subject.subject') = subject.id)
 OR (json_extract(content, '$.record.record.subject.kind') = 'task' AND json_extract(content, '$.record.record.subject.subject') IN (SELECT task_id FROM tasks WHERE workstream_id = subject.id))))
OR (record_kind = 'roleAssignment' AND EXISTS (SELECT 1 FROM role_assignments AS assignment WHERE assignment.assignment_id = project_feed.record_id AND assignment.workstream_subject = subject.id))
OR (record_kind = 'fact' AND (
 EXISTS (SELECT 1 FROM json_each(json_extract(content, '$.record.record.links')) AS link WHERE json_extract(link.value, '$.kind') = subject.kind AND json_extract(link.value, '$.id') = subject.id)
 OR (json_extract(content, '$.record.record.kind.kind') = 'evaluatorResult' AND json_extract(content, '$.record.record.kind.fact.subject.kind') = subject.kind AND json_extract(content, '$.record.record.kind.fact.subject.id') = subject.id)
 ))
OR (record_kind = 'notifyIntent' AND EXISTS (SELECT 1 FROM notify_intents AS intent WHERE intent.intent_id = project_feed.record_id AND intent.cause_kind = subject.kind AND intent.cause_id = subject.id))
OR (record_kind = 'request' AND json_extract(content, '$.record.record.decision.kind') = 'accepted' AND EXISTS (SELECT 1 FROM project_feed AS effect INDEXED BY feed_by_request_position WHERE effect.request_id = project_feed.request_id AND effect.record_kind = subject.kind AND effect.record_id = subject.id AND effect.record_kind <> 'request'))
) ORDER BY position DESC LIMIT 50
"#, id, start, end)
                .fetch_all(&mut self.transaction).await?.into_iter().map(|row| UnreadMetadataRow { kind: row.record_kind, id: row.record_id, count: row.unread_count }).collect(),
            _ => sqlx_turso::query!(r#"
WITH subject AS (SELECT ? AS kind, ? AS id)
SELECT record_kind AS "record_kind!: String", record_id AS "record_id!: String", COUNT(*) OVER () AS "unread_count!: i64" FROM project_feed, subject
WHERE position > ? AND position <= ? AND (
(record_kind = subject.kind AND record_id = subject.id)
OR (record_kind = 'fact' AND (
 EXISTS (SELECT 1 FROM json_each(json_extract(content, '$.record.record.links')) AS link WHERE json_extract(link.value, '$.kind') = subject.kind AND json_extract(link.value, '$.id') = subject.id)
 OR (json_extract(content, '$.record.record.kind.kind') = 'evaluatorResult' AND json_extract(content, '$.record.record.kind.fact.subject.kind') = subject.kind AND json_extract(content, '$.record.record.kind.fact.subject.id') = subject.id)
 ))
OR (record_kind = 'notifyIntent' AND EXISTS (SELECT 1 FROM notify_intents AS intent WHERE intent.intent_id = project_feed.record_id AND intent.cause_kind = subject.kind AND intent.cause_id = subject.id))
OR (record_kind = 'request' AND json_extract(content, '$.record.record.decision.kind') = 'accepted' AND EXISTS (SELECT 1 FROM project_feed AS effect INDEXED BY feed_by_request_position WHERE effect.request_id = project_feed.request_id AND effect.record_kind = subject.kind AND effect.record_id = subject.id AND effect.record_kind <> 'request'))
) ORDER BY position DESC LIMIT 50
"#, kind, id, start, end)
                .fetch_all(&mut self.transaction).await?.into_iter().map(|row| UnreadMetadataRow { kind: row.record_kind, id: row.record_id, count: row.unread_count }).collect(),
        };
        let unread_count = rows.first().map_or(Ok(0), |row| {
            u64::try_from(row.count).map_err(|_| ProjectStorageError::InvalidFeed)
        })?;
        let unread = rows
            .into_iter()
            .map(|row| {
                let kind = serde_json::from_value(serde_json::Value::String(row.kind))
                    .map_err(|_| ProjectStorageError::InvalidFeed)?;
                let id =
                    RecordId::try_from(row.id).map_err(|_| ProjectStorageError::InvalidFeed)?;
                Ok(RecordRef { kind, id })
            })
            .collect::<Result<Vec<_>, ProjectStorageError>>()?;
        Ok(ReaderState {
            subject: subject.clone(),
            read_through: after,
            unread_count,
            unread,
        })
    }
}
