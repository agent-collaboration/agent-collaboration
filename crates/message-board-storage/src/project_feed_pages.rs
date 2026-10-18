//! Feed candidates are metadata; bodies are fetched in one batch after the key merge.
use crate::project_record_io::{validate_canonical_project_text, validate_project_record};
use crate::{ProjectSnapshot, ProjectStorageError};
use chrono::{DateTime, Utc};
use message_board::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct FeedPageKey {
    pub cursor: FeedCursor,
    pub reference: RecordRef,
    pub router_time: DateTime<Utc>,
}
#[derive(Clone, Copy)]
pub enum FeedPageDirection {
    Forward,
    Backward,
}
pub struct FeedPageBounds {
    pub after: FeedPosition,
    pub through: FeedPosition,
    pub direction: FeedPageDirection,
    pub limit: u64,
    /// Only a newly in-scope project applies this cut; known projects use positions.
    pub sweep: Option<MergeKey>,
}
pub enum PostPageScope<'scope> {
    All,
    Main,
    Thread(&'scope ThreadId),
}
pub struct PostPageFilter<'scope> {
    pub scope: PostPageScope<'scope>,
    pub search: Option<(&'scope str, SearchPostKind)>,
}
pub struct StoredFeedEntry {
    project: ProjectId,
    key: FeedPageKey,
    payload: String,
    redacted: Option<bool>,
}
impl StoredFeedEntry {
    pub fn decode(self, include_redacted: bool) -> Result<FeedEntry, ProjectStorageError> {
        tracing::trace!(target: "message_board_storage::decode", decode_kind = "feed");
        let mut entry: FeedEntry =
            serde_json::from_str(&self.payload).map_err(|_| ProjectStorageError::InvalidFeed)?;
        validate_canonical_project_text(&self.payload, &entry, &entry.record)?;
        if entry.position != self.key.cursor.position
            || entry.epoch != self.key.cursor.epoch
            || entry.record != self.key.reference
            || entry.router_time != self.key.router_time
        {
            return Err(ProjectStorageError::InvalidFeed);
        }
        if let FeedContent::Shown(record) = &mut entry.content {
            validate_project_record(record, &self.project)?;
            let envelope = record.envelope();
            if record.reference() != entry.record
                || envelope.id != entry.record.id
                || envelope.position != RecordPosition::Project(self.key.cursor)
                || envelope.scope.project.as_ref() != Some(&self.project)
                || envelope.actor != entry.actor
                || envelope.request != entry.request
                || envelope.router_time != entry.router_time
            {
                return Err(ProjectStorageError::InvalidFeed);
            }
            if let Record::WorkTrailRow(row) = record.as_mut() {
                row.redacted = self.redacted.ok_or(ProjectStorageError::InvalidFeed)?;
                if row.redacted && !include_redacted {
                    entry.content = FeedContent::Redacted;
                }
            }
        }
        Ok(entry)
    }
}
impl ProjectSnapshot {
    /// Validate only metadata across the scanned interval, including gaps between hits.
    pub async fn validate_feed_range(
        &mut self,
        after: FeedPosition,
        through: FeedPosition,
    ) -> Result<(), ProjectStorageError> {
        let start = i64::try_from(after.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let end = i64::try_from(through.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        if end < start || through > self.high_water.position {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let row = sqlx_turso::query!(r#"SELECT COUNT(*) AS "count!: i64", MIN(epoch) AS "minimum: i64", MAX(epoch) AS "maximum: i64" FROM project_feed WHERE position > ? AND position <= ?"#, start, end).fetch_one(&mut *self.transaction).await?;
        let epoch = i64::try_from(self.high_water.epoch.get())
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        if row.count != end - start
            || (row.count > 0 && (row.minimum != Some(epoch) || row.maximum != Some(epoch)))
        {
            return Err(ProjectStorageError::InvalidFeed);
        }
        Ok(())
    }
    pub async fn feed_entry_batch(
        &mut self,
        keys: &[FeedPageKey],
    ) -> Result<BTreeMap<FeedPosition, StoredFeedEntry>, ProjectStorageError> {
        if keys.is_empty() {
            return Ok(BTreeMap::new());
        }
        let positions = serde_json::to_string(
            &keys
                .iter()
                .map(|key| key.cursor.position.get())
                .collect::<Vec<_>>(),
        )?;
        let rows = sqlx_turso::query!(r#"SELECT f.position AS "position!: i64", f.entry AS "entry!: String", t.redacted AS "redacted: bool" FROM json_each(?) k CROSS JOIN project_feed f ON f.position = k.value LEFT JOIN work_trail_rows t ON f.record_kind = 'workTrailRow' AND t.row_id = f.record_id ORDER BY f.position"#, positions).fetch_all(&mut *self.transaction).await?;
        if rows.len() != keys.len() {
            return Err(ProjectStorageError::InvalidFeed);
        }
        rows.into_iter()
            .map(|row| {
                let position = FeedPosition::try_from(
                    u64::try_from(row.position).map_err(|_| ProjectStorageError::InvalidFeed)?,
                )
                .map_err(|_| ProjectStorageError::InvalidFeed)?;
                let key = keys
                    .iter()
                    .find(|key| key.cursor.position == position)
                    .cloned()
                    .ok_or(ProjectStorageError::InvalidFeed)?;
                Ok((
                    position,
                    StoredFeedEntry {
                        project: self.project_id.clone(),
                        key,
                        payload: row.entry,
                        redacted: row.redacted,
                    },
                ))
            })
            .collect()
    }
    pub async fn post_page_keys(
        &mut self,
        bounds: &FeedPageBounds,
        filter: PostPageFilter<'_>,
    ) -> Result<Vec<FeedPageKey>, ProjectStorageError> {
        let (main, thread) = match filter.scope {
            PostPageScope::All => (false, None),
            PostPageScope::Main => (true, None),
            PostPageScope::Thread(id) => (false, Some(id.as_str())),
        };
        let (query, kind) = filter.search.map_or((None, "both"), |(query, kind)| {
            (
                Some(query),
                match kind {
                    SearchPostKind::Both => "both",
                    SearchPostKind::TopLevel => "topLevel",
                    SearchPostKind::Thread => "thread",
                },
            )
        });
        let start =
            i64::try_from(bounds.after.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let end =
            i64::try_from(bounds.through.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let limit = i64::try_from(bounds.limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let sweep = bounds
            .sweep
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let project = self.project_id.as_str();
        macro_rules! decode_rows {
            ($rows:expr) => {
                $rows
                    .into_iter()
                    .map(|row| {
                        self.feed_page_key(
                            row.position,
                            row.epoch,
                            "post".to_owned(),
                            row.record_id,
                            row.router_time,
                        )
                    })
                    .collect()
            };
        }
        match bounds.direction {
            FeedPageDirection::Forward => decode_rows!(
                sqlx_turso::query_file!(
                    "src/project_post_page.sql",
                    sweep,
                    project,
                    start,
                    end,
                    main,
                    thread,
                    thread,
                    thread,
                    query,
                    query,
                    kind,
                    kind,
                    kind,
                    limit
                )
                .fetch_all(&mut *self.transaction)
                .await?
            ),
            FeedPageDirection::Backward => decode_rows!(
                sqlx_turso::query_file!(
                    "src/project_post_page_backward.sql",
                    sweep,
                    project,
                    start,
                    end,
                    main,
                    thread,
                    thread,
                    thread,
                    query,
                    query,
                    kind,
                    kind,
                    kind,
                    limit
                )
                .fetch_all(&mut *self.transaction)
                .await?
            ),
        }
    }

    pub(crate) fn feed_page_key(
        &self,
        position: i64,
        epoch: i64,
        kind: String,
        id: String,
        time: String,
    ) -> Result<FeedPageKey, ProjectStorageError> {
        let invalid = || ProjectStorageError::InvalidFeed;
        let cursor = FeedCursor {
            epoch: EpochNumber::try_from(u64::try_from(epoch).map_err(|_| invalid())?)
                .map_err(|_| invalid())?,
            position: FeedPosition::try_from(u64::try_from(position).map_err(|_| invalid())?)
                .map_err(|_| invalid())?,
        };
        if cursor.epoch != self.high_water.epoch || cursor.position > self.high_water.position {
            return Err(invalid());
        }
        Ok(FeedPageKey {
            cursor,
            reference: RecordRef {
                kind: serde_json::from_value(serde_json::Value::String(kind))
                    .map_err(|_| invalid())?,
                id: RecordId::try_from(id).map_err(|_| invalid())?,
            },
            router_time: time.parse().map_err(|_| invalid())?,
        })
    }
}
