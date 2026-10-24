//! View operands are current records and small metadata projections, not an event replay.
use crate::project_record_io::record_kind_tag;
use crate::{ProjectSnapshot, ProjectStorageError};
use message_board::*;
use std::collections::BTreeMap;
impl ProjectSnapshot {
    pub async fn record_counts(
        &mut self,
    ) -> Result<BTreeMap<RecordKind, u64>, ProjectStorageError> {
        let rows = sqlx_turso::query!(r#"SELECT kind AS "kind!: String", count AS "count!: i64" FROM (SELECT 'project' AS kind, COUNT(*) AS count FROM project_state UNION ALL SELECT 'workstream' AS kind, COUNT(*) AS count FROM workstreams UNION ALL SELECT 'task' AS kind, COUNT(*) AS count FROM tasks UNION ALL SELECT 'subtask' AS kind, COUNT(*) AS count FROM subtasks UNION ALL SELECT 'taskLinkEvent' AS kind, COUNT(*) AS count FROM task_link_events UNION ALL SELECT 'thread' AS kind, COUNT(*) AS count FROM threads UNION ALL SELECT 'post' AS kind, COUNT(*) AS count FROM posts UNION ALL SELECT 'threadRefiled' AS kind, COUNT(*) AS count FROM thread_refilings UNION ALL SELECT 'roleAssignment' AS kind, COUNT(*) AS count FROM role_assignments UNION ALL SELECT 'ownerGrant' AS kind, COUNT(*) AS count FROM owner_grants UNION ALL SELECT 'standDownAcknowledgement' AS kind, COUNT(*) AS count FROM stand_down_acknowledgements UNION ALL SELECT 'fact' AS kind, COUNT(*) AS count FROM facts UNION ALL SELECT 'move' AS kind, COUNT(*) AS count FROM moves UNION ALL SELECT 'planRevision' AS kind, COUNT(*) AS count FROM plan_revisions UNION ALL SELECT 'workTrailRow' AS kind, COUNT(*) AS count FROM work_trail_rows UNION ALL SELECT 'request' AS kind, COUNT(*) AS count FROM request_decisions UNION ALL SELECT 'notifyIntent' AS kind, COUNT(*) AS count FROM notify_intents UNION ALL SELECT 'subscriptionMembership' AS kind, COUNT(*) AS count FROM subscription_memberships UNION ALL SELECT 'readMark' AS kind, COUNT(*) AS count FROM read_marks UNION ALL SELECT 'trailRedaction' AS kind,COUNT(*) AS count FROM project_feed WHERE record_kind='trailRedaction') WHERE count>0 ORDER BY kind"#).fetch_all(&mut self.transaction).await?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    serde_json::from_value(serde_json::Value::String(row.kind))
                        .map_err(|_| ProjectStorageError::InvalidFeed)?,
                    u64::try_from(row.count).map_err(|_| ProjectStorageError::InvalidFeed)?,
                ))
            })
            .collect()
    }
    pub async fn view_kind_page_keys(
        &mut self,
        kind: RecordKind,
        after: &str,
        include_removed: bool,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        let rows = match kind {
            RecordKind::Subtask if !include_removed => sqlx_turso::query_scalar!(r#"SELECT subtask_id AS "id!: String" FROM subtasks WHERE removed=0 AND subtask_id>? ORDER BY subtask_id LIMIT 101"#, after).fetch_all(&mut self.transaction).await?,
            RecordKind::RoleAssignment => sqlx_turso::query_scalar!(r#"SELECT assignment_id AS "id!: String" FROM role_assignments WHERE ended IS NULL AND assignment_id>? ORDER BY assignment_id LIMIT 101"#, after).fetch_all(&mut self.transaction).await?,
            RecordKind::Move => sqlx_turso::query_scalar!(r#"SELECT move_id AS "id!: String" FROM moves WHERE move_id>? AND position>=COALESCE((SELECT MAX(position) FROM moves WHERE json_extract(outcome,'$.kind')='accepted'),0) ORDER BY move_id LIMIT 101"#, after).fetch_all(&mut self.transaction).await?,
            _ => return self.record_keys(kind, after, 101).await,
        };
        rows.into_iter()
            .map(|row| {
                Ok(RecordRef {
                    kind,
                    id: RecordId::try_from(row).map_err(|_| ProjectStorageError::InvalidFeed)?,
                })
            })
            .collect()
    }
    pub async fn creation_metadata(
        &mut self,
        references: &[RecordRef],
    ) -> Result<Vec<crate::FeedPageKey>, ProjectStorageError> {
        let keys = serde_json::to_string(
            &references
                .iter()
                .map(|key| (record_kind_tag(key.kind), key.id.as_str()))
                .collect::<Vec<_>>(),
        )?;
        let rows = sqlx_turso::query!(r#"SELECT f.record_kind AS "kind!: String", f.record_id AS "id!: String", f.position AS "position!: i64", f.epoch AS "epoch!: i64", f.router_time AS "time?: String", f.time_basis AS "time_basis!: String" FROM json_each(?) k CROSS JOIN project_feed f ON f.position = (SELECT MIN(first.position) FROM project_feed first WHERE first.record_kind = json_extract(k.value, '$[0]') AND first.record_id = json_extract(k.value, '$[1]')) ORDER BY f.record_kind, f.record_id"#, keys).fetch_all(&mut self.transaction).await?;
        rows.into_iter()
            .map(|row| {
                self.feed_page_key(
                    row.position,
                    row.epoch,
                    row.kind,
                    row.id,
                    row.time,
                    row.time_basis,
                )
            })
            .collect()
    }
    pub async fn recent_trail_keys(
        &mut self,
        subject: &TrailSubject,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        let subject = serde_json::to_string(subject)?;
        let rows = sqlx_turso::query_file!("src/project_view_recent_trail.sql", subject)
            .fetch_all(&mut self.transaction)
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(RecordRef {
                    kind: RecordKind::WorkTrailRow,
                    id: RecordId::try_from(row.id).map_err(|_| ProjectStorageError::InvalidFeed)?,
                })
            })
            .collect()
    }
    pub async fn card_intent_page_keys(
        &mut self,
        subject: &TrailSubject,
        after: &str,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        let subject = serde_json::to_string(subject)?;
        let rows = sqlx_turso::query_file!("src/project_view_card_intents.sql", subject, after)
            .fetch_all(&mut self.transaction)
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(RecordRef {
                    kind: RecordKind::NotifyIntent,
                    id: RecordId::try_from(row.id).map_err(|_| ProjectStorageError::InvalidFeed)?,
                })
            })
            .collect()
    }
}
