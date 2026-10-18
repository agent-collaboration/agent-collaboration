//! View operands are current records and small metadata projections, not an event replay.
use crate::project_record_io::record_kind_tag;
use crate::{ProjectSnapshot, ProjectStorageError};
use message_board::*;
use std::collections::BTreeMap;
impl ProjectSnapshot {
    pub async fn record_counts(
        &mut self,
    ) -> Result<BTreeMap<RecordKind, u64>, ProjectStorageError> {
        let rows = sqlx_turso::query!(r#"SELECT kind AS "kind!: String", COUNT(*) AS "count!: i64" FROM record_identities GROUP BY kind ORDER BY kind"#).fetch_all(&mut *self.transaction).await?;
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
        let tag = record_kind_tag(kind);
        let rows = sqlx_turso::query_file!(
            "src/project_view_kind_page.sql",
            tag,
            after,
            include_removed
        )
        .fetch_all(&mut *self.transaction)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(RecordRef {
                    kind,
                    id: RecordId::try_from(row.id).map_err(|_| ProjectStorageError::InvalidFeed)?,
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
        let rows = sqlx_turso::query!(r#"SELECT f.record_kind AS "kind!: String", f.record_id AS "id!: String", f.position AS "position!: i64", f.epoch AS "epoch!: i64", json_extract(f.entry, '$.routerTime') AS "time!: String" FROM json_each(?) k CROSS JOIN project_feed f ON f.position = (SELECT MIN(first.position) FROM project_feed first WHERE first.record_kind = json_extract(k.value, '$[0]') AND first.record_id = json_extract(k.value, '$[1]')) ORDER BY f.record_kind, f.record_id"#, keys).fetch_all(&mut *self.transaction).await?;
        rows.into_iter()
            .map(|row| self.feed_page_key(row.position, row.epoch, row.kind, row.id, row.time))
            .collect()
    }
    pub async fn recent_trail_keys(
        &mut self,
        subject: &TrailSubject,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        let subject = serde_json::to_string(subject)?;
        let rows = sqlx_turso::query_file!("src/project_view_recent_trail.sql", subject)
            .fetch_all(&mut *self.transaction)
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
            .fetch_all(&mut *self.transaction)
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
