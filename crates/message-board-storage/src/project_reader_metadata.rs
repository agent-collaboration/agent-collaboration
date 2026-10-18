//! Reader-state counts and bounded references use existing feed and subject metadata.
use crate::{ProjectSnapshot, ProjectStorageError, project_record_io::record_kind_tag};
use message_board::*;

struct UnreadMetadataRow {
    kind: String,
    id: String,
    count: i64,
}

/// Identity fields required to bind a metadata-only subject lookup to its physical store.
struct StoredSubjectIdentity {
    kind: Option<String>,
    id: Option<String>,
    envelope_id: Option<String>,
    project: Option<String>,
}
impl StoredSubjectIdentity {
    fn validate(
        &self,
        reference: &RecordRef,
        project: &ProjectId,
    ) -> Result<(), ProjectStorageError> {
        let invalid = |field| ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        };
        if self.kind.as_deref() != Some(record_kind_tag(reference.kind)) {
            return Err(invalid("kind"));
        }
        if self.id.as_deref() != Some(reference.id.as_str())
            || self.envelope_id.as_deref() != Some(reference.id.as_str())
        {
            return Err(invalid("id"));
        }
        if self.project.as_deref() != Some(project.as_str()) {
            return Err(invalid("scope"));
        }
        Ok(())
    }
}

impl ProjectSnapshot {
    pub async fn subtask_task_metadata(
        &mut self,
        subtask: &SubtaskId,
    ) -> Result<Option<TaskId>, ProjectStorageError> {
        let id = subtask.as_str();
        let row=sqlx_turso::query!(r#"SELECT
CASE WHEN json_type(payload,'$.kind')='text' THEN json_extract(payload,'$.kind') END AS "kind?: String",
CASE WHEN json_type(payload,'$.record.subtaskId')='text' THEN json_extract(payload,'$.record.subtaskId') END AS "id?: String",
CASE WHEN json_type(payload,'$.record.envelope.id')='text' THEN json_extract(payload,'$.record.envelope.id') END AS "envelope_id?: String",
CASE WHEN json_type(payload,'$.record.envelope.scope.project')='text' THEN json_extract(payload,'$.record.envelope.scope.project') END AS "project?: String",
CASE WHEN json_type(payload,'$.record.taskId')='text' THEN json_extract(payload,'$.record.taskId') END AS "task?: String"
FROM record_identities WHERE kind='subtask' AND id=?"#,id).fetch_optional(&mut *self.transaction).await?;
        row.map(|row| {
            let reference = subtask.clone().into();
            StoredSubjectIdentity {
                kind: row.kind,
                id: row.id,
                envelope_id: row.envelope_id,
                project: row.project,
            }
            .validate(&reference, &self.project_id)?;
            row.task.and_then(|task| TaskId::try_from(task).ok()).ok_or(
                ProjectStorageError::InvalidRecord {
                    record: reference,
                    field: "taskId",
                },
            )
        })
        .transpose()
    }

    pub async fn trail_subject_metadata(
        &mut self,
        trail: &WorkTrailRowId,
    ) -> Result<Option<TrailSubject>, ProjectStorageError> {
        let id = trail.as_str();
        let row=sqlx_turso::query!(r#"SELECT
CASE WHEN json_type(payload,'$.kind')='text' THEN json_extract(payload,'$.kind') END AS "kind?: String",
CASE WHEN json_type(payload,'$.record.rowId')='text' THEN json_extract(payload,'$.record.rowId') END AS "id?: String",
CASE WHEN json_type(payload,'$.record.envelope.id')='text' THEN json_extract(payload,'$.record.envelope.id') END AS "envelope_id?: String",
CASE WHEN json_type(payload,'$.record.envelope.scope.project')='text' THEN json_extract(payload,'$.record.envelope.scope.project') END AS "project?: String",
CASE WHEN json_type(payload,'$.record.projectId')='text' THEN json_extract(payload,'$.record.projectId') END AS "body_project?: String",
CASE WHEN json_type(payload,'$.record.subject')='object' THEN json_extract(payload,'$.record.subject') END AS "subject?: String"
FROM record_identities WHERE kind='workTrailRow' AND id=?"#,id).fetch_optional(&mut *self.transaction).await?;
        row.map(|row| {
            let reference = trail.clone().into();
            StoredSubjectIdentity {
                kind: row.kind,
                id: row.id,
                envelope_id: row.envelope_id,
                project: row.project,
            }
            .validate(&reference, &self.project_id)?;
            if row.body_project.as_deref() != Some(self.project_id.as_str()) {
                return Err(ProjectStorageError::InvalidRecord {
                    record: reference,
                    field: "project",
                });
            }
            row.subject
                .and_then(|subject| serde_json::from_str::<TrailSubject>(&subject).ok())
                .ok_or(ProjectStorageError::InvalidRecord {
                    record: reference,
                    field: "subject",
                })
        })
        .transpose()
    }

    pub async fn unread_state_after(
        &mut self,
        subject: &RecordRef,
        after: FeedCursor,
    ) -> Result<ReaderState, ProjectStorageError> {
        let high = self.high_water();
        if after.epoch != high.epoch || after.position > high.position {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let start =
            i64::try_from(after.position.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let end =
            i64::try_from(high.position.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        // A position primary key plus an exact range count proves density. Epoch extrema
        // check every row in the scanned range, including unrelated subjects.
        let line = sqlx_turso::query!(r#"SELECT COUNT(*) AS "row_count!: i64", MIN(epoch) AS "minimum_epoch?: i64", MAX(epoch) AS "maximum_epoch?: i64" FROM project_feed WHERE position > ? AND position <= ?"#, start, end).fetch_one(&mut *self.transaction).await?;
        let epoch =
            i64::try_from(high.epoch.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        if line.row_count != end - start
            || (line.row_count > 0
                && (line.minimum_epoch != Some(epoch) || line.maximum_epoch != Some(epoch)))
        {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let kind = record_kind_tag(subject.kind);
        let id = subject.id.as_str();
        // Separate checked statements bound native parser depth to the requested subject.
        // No body is transferred or domain-decoded for an unread count. Retained child
        // entries carry their task even after checklist removal; task ancestry follows
        // the existing task relation. A project subject belongs only to this snapshot.
        let rows: Vec<UnreadMetadataRow> = match subject.kind {
            RecordKind::Project if id != self.project_id.as_str() => Vec::new(),
            RecordKind::Project => sqlx_turso::query!(r#"SELECT record_kind AS "record_kind!: String", record_id AS "record_id!: String", COUNT(*) OVER () AS "unread_count!: i64" FROM project_feed WHERE position > ? AND position <= ? ORDER BY position DESC LIMIT 50"#, start, end)
                .fetch_all(&mut *self.transaction).await?.into_iter().map(|row| UnreadMetadataRow { kind: row.record_kind, id: row.record_id, count: row.unread_count }).collect(),
            RecordKind::Thread => sqlx_turso::query!(r#"
WITH subject AS (SELECT 'thread' AS kind, ? AS id)
SELECT record_kind AS "record_kind!: String", record_id AS "record_id!: String", COUNT(*) OVER () AS "unread_count!: i64" FROM project_feed, subject
WHERE position > ? AND position <= ? AND (
(record_kind = subject.kind AND record_id = subject.id)
OR (record_kind = 'threadRefiled' AND EXISTS (SELECT 1 FROM thread_refilings AS refile WHERE refile.refiling_id = project_feed.record_id AND refile.thread_id = subject.id))
OR (record_kind = 'post' AND (record_id = subject.id OR EXISTS (SELECT 1 FROM posts AS post WHERE post.post_id = project_feed.record_id AND post.thread_id = subject.id)))
OR (record_kind = 'fact' AND (
 EXISTS (SELECT 1 FROM json_each(json_extract(entry, '$.content.record.record.links')) AS link WHERE json_extract(link.value, '$.kind') = subject.kind AND json_extract(link.value, '$.id') = subject.id)
 OR (json_extract(entry, '$.content.record.record.kind.kind') = 'evaluatorResult' AND json_extract(entry, '$.content.record.record.kind.fact.subject.kind') = subject.kind AND json_extract(entry, '$.content.record.record.kind.fact.subject.id') = subject.id)
 OR (json_extract(entry, '$.content.record.record.kind.kind') = 'lifecycle' AND json_extract(entry, '$.content.record.record.kind.fact.kind') = 'threadResolved' AND json_extract(entry, '$.content.record.record.kind.fact.thread') = subject.id)))
OR (record_kind = 'notifyIntent' AND EXISTS (SELECT 1 FROM notify_intents AS intent WHERE intent.intent_id = project_feed.record_id AND intent.cause_kind = subject.kind AND intent.cause_id = subject.id))
OR (record_kind = 'request' AND json_extract(entry, '$.content.record.record.decision.kind') = 'accepted' AND EXISTS (SELECT 1 FROM json_each(json_extract(entry, '$.content.record.record.decision.records')) AS accepted WHERE json_extract(accepted.value, '$.kind') = subject.kind AND json_extract(accepted.value, '$.id') = subject.id))
) ORDER BY position DESC LIMIT 50
"#, id, start, end)
                .fetch_all(&mut *self.transaction).await?.into_iter().map(|row| UnreadMetadataRow { kind: row.record_kind, id: row.record_id, count: row.unread_count }).collect(),
            RecordKind::Task => sqlx_turso::query!(r#"
WITH subject AS (SELECT 'task' AS kind, ? AS id)
SELECT record_kind AS "record_kind!: String", record_id AS "record_id!: String", COUNT(*) OVER () AS "unread_count!: i64" FROM project_feed, subject
WHERE position > ? AND position <= ? AND (
(record_kind = subject.kind AND record_id = subject.id)
OR (record_kind = 'subtask' AND json_extract(entry, '$.content.record.record.taskId') = subject.id)
OR (record_kind = 'taskLinkEvent' AND json_extract(entry, '$.content.record.record.change.link.from') = subject.id)
OR (record_kind = 'workTrailRow' AND json_extract(entry, '$.content.record.record.subject.kind') = 'task' AND json_extract(entry, '$.content.record.record.subject.subject') = subject.id)
OR (record_kind = 'fact' AND (
 EXISTS (SELECT 1 FROM json_each(json_extract(entry, '$.content.record.record.links')) AS link WHERE json_extract(link.value, '$.kind') = subject.kind AND json_extract(link.value, '$.id') = subject.id)
 OR (json_extract(entry, '$.content.record.record.kind.kind') = 'evaluatorResult' AND json_extract(entry, '$.content.record.record.kind.fact.subject.kind') = subject.kind AND json_extract(entry, '$.content.record.record.kind.fact.subject.id') = subject.id)
 OR (json_extract(entry, '$.content.record.record.kind.kind') = 'lifecycle' AND json_extract(entry, '$.content.record.record.kind.fact.kind') = 'dropped' AND json_extract(entry, '$.content.record.record.kind.fact.task') = subject.id)))
OR (record_kind = 'notifyIntent' AND EXISTS (SELECT 1 FROM notify_intents AS intent WHERE intent.intent_id = project_feed.record_id AND intent.cause_kind = subject.kind AND intent.cause_id = subject.id))
OR (record_kind = 'request' AND json_extract(entry, '$.content.record.record.decision.kind') = 'accepted' AND EXISTS (SELECT 1 FROM json_each(json_extract(entry, '$.content.record.record.decision.records')) AS accepted WHERE json_extract(accepted.value, '$.kind') = subject.kind AND json_extract(accepted.value, '$.id') = subject.id))
) ORDER BY position DESC LIMIT 50
"#, id, start, end)
                .fetch_all(&mut *self.transaction).await?.into_iter().map(|row| UnreadMetadataRow { kind: row.record_kind, id: row.record_id, count: row.unread_count }).collect(),
            RecordKind::Workstream => sqlx_turso::query!(r#"
WITH subject AS (SELECT 'workstream' AS kind, ? AS id)
SELECT record_kind AS "record_kind!: String", record_id AS "record_id!: String", COUNT(*) OVER () AS "unread_count!: i64" FROM project_feed, subject
WHERE position > ? AND position <= ? AND (
(record_kind = subject.kind AND record_id = subject.id)
OR (record_kind = 'task' AND json_extract(entry, '$.content.record.record.workstreamId') = subject.id)
OR (record_kind = 'subtask' AND EXISTS (SELECT 1 FROM tasks AS task WHERE task.task_id = json_extract(entry, '$.content.record.record.taskId') AND task.workstream_id = subject.id))
OR (record_kind = 'taskLinkEvent' AND EXISTS (SELECT 1 FROM tasks AS task WHERE task.task_id = json_extract(entry, '$.content.record.record.change.link.from') AND task.workstream_id = subject.id))
OR (record_kind = 'workTrailRow' AND (
 (json_extract(entry, '$.content.record.record.subject.kind') = 'workstream' AND json_extract(entry, '$.content.record.record.subject.subject') = subject.id)
 OR (json_extract(entry, '$.content.record.record.subject.kind') = 'task' AND json_extract(entry, '$.content.record.record.subject.subject') IN (SELECT task_id FROM tasks WHERE workstream_id = subject.id))))
OR (record_kind = 'roleAssignment' AND EXISTS (SELECT 1 FROM role_assignments AS assignment WHERE assignment.assignment_id = project_feed.record_id AND assignment.workstream_subject = subject.id))
OR (record_kind = 'fact' AND (
 EXISTS (SELECT 1 FROM json_each(json_extract(entry, '$.content.record.record.links')) AS link WHERE json_extract(link.value, '$.kind') = subject.kind AND json_extract(link.value, '$.id') = subject.id)
 OR (json_extract(entry, '$.content.record.record.kind.kind') = 'evaluatorResult' AND json_extract(entry, '$.content.record.record.kind.fact.subject.kind') = subject.kind AND json_extract(entry, '$.content.record.record.kind.fact.subject.id') = subject.id)
 ))
OR (record_kind = 'notifyIntent' AND EXISTS (SELECT 1 FROM notify_intents AS intent WHERE intent.intent_id = project_feed.record_id AND intent.cause_kind = subject.kind AND intent.cause_id = subject.id))
OR (record_kind = 'request' AND json_extract(entry, '$.content.record.record.decision.kind') = 'accepted' AND EXISTS (SELECT 1 FROM json_each(json_extract(entry, '$.content.record.record.decision.records')) AS accepted WHERE json_extract(accepted.value, '$.kind') = subject.kind AND json_extract(accepted.value, '$.id') = subject.id))
) ORDER BY position DESC LIMIT 50
"#, id, start, end)
                .fetch_all(&mut *self.transaction).await?.into_iter().map(|row| UnreadMetadataRow { kind: row.record_kind, id: row.record_id, count: row.unread_count }).collect(),
            _ => sqlx_turso::query!(r#"
WITH subject AS (SELECT ? AS kind, ? AS id)
SELECT record_kind AS "record_kind!: String", record_id AS "record_id!: String", COUNT(*) OVER () AS "unread_count!: i64" FROM project_feed, subject
WHERE position > ? AND position <= ? AND (
(record_kind = subject.kind AND record_id = subject.id)
OR (record_kind = 'fact' AND (
 EXISTS (SELECT 1 FROM json_each(json_extract(entry, '$.content.record.record.links')) AS link WHERE json_extract(link.value, '$.kind') = subject.kind AND json_extract(link.value, '$.id') = subject.id)
 OR (json_extract(entry, '$.content.record.record.kind.kind') = 'evaluatorResult' AND json_extract(entry, '$.content.record.record.kind.fact.subject.kind') = subject.kind AND json_extract(entry, '$.content.record.record.kind.fact.subject.id') = subject.id)
 ))
OR (record_kind = 'notifyIntent' AND EXISTS (SELECT 1 FROM notify_intents AS intent WHERE intent.intent_id = project_feed.record_id AND intent.cause_kind = subject.kind AND intent.cause_id = subject.id))
OR (record_kind = 'request' AND json_extract(entry, '$.content.record.record.decision.kind') = 'accepted' AND EXISTS (SELECT 1 FROM json_each(json_extract(entry, '$.content.record.record.decision.records')) AS accepted WHERE json_extract(accepted.value, '$.kind') = subject.kind AND json_extract(accepted.value, '$.id') = subject.id))
) ORDER BY position DESC LIMIT 50
"#, kind, id, start, end)
                .fetch_all(&mut *self.transaction).await?.into_iter().map(|row| UnreadMetadataRow { kind: row.record_kind, id: row.record_id, count: row.unread_count }).collect(),
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
