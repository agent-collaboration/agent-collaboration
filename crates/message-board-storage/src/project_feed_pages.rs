//! Feed candidates are metadata; bodies are fetched in one batch after the key merge.
use crate::{ProjectSnapshot, ProjectStorageError};
use message_board::{
    EpochNumber, FeedCursor, FeedEntry, FeedPosition, MergeKey, ProjectId, RecordId, RecordKind,
    RecordRef, RecordTime, SearchPostKind, ThreadId,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct FeedPageKey {
    pub cursor: FeedCursor,
    pub reference: RecordRef,
    pub time: RecordTime,
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
    row: crate::project_feed_io::ProjectFeedRow,
}
impl StoredFeedEntry {
    pub fn decode(self, include_redacted: bool) -> Result<FeedEntry, ProjectStorageError> {
        let entry = self
            .row
            .decode(&self.project, include_redacted)?
            .ok_or(ProjectStorageError::InvalidFeed)?;
        if entry.position != self.key.cursor.position
            || entry.epoch != self.key.cursor.epoch
            || entry.record != self.key.reference
            || entry.time != self.key.time
        {
            return Err(ProjectStorageError::InvalidFeed);
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
        // Keep every check in this pinned read transaction, but avoid nesting a
        // successor SELECT inside CASE and COUNT (UW-3). Only epochs represented
        // in this range need one indexed successor lookup; no full history scan.
        let row = sqlx_turso::query!(r#"SELECT COUNT(*) AS "count!: i64", COUNT(DISTINCT position) AS "distinct_count!: i64" FROM project_feed WHERE position>? AND position<=?"#, start, end)
            .fetch_one(&mut self.transaction).await?;
        if row.count != end - start || row.distinct_count != row.count {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let epochs = sqlx_turso::query!(r#"SELECT f.epoch AS "epoch!: i64", MIN(f.position) AS "first!: i64", MAX(f.position) AS "last!: i64", e.line_id AS "line_id?: String", e.inherited_through AS "inherited_through?: i64" FROM project_feed f LEFT JOIN epoch_lines e ON e.epoch=f.epoch WHERE f.position>? AND f.position<=? GROUP BY f.epoch, e.line_id, e.inherited_through"#, start, end)
            .fetch_all(&mut self.transaction).await?;
        for epoch in epochs {
            let (Some(_), Some(inherited)) = (epoch.line_id, epoch.inherited_through) else {
                return Err(ProjectStorageError::InvalidFeed);
            };
            let successor = sqlx_turso::query!(r#"SELECT inherited_through AS "inherited_through!: i64" FROM epoch_lines WHERE epoch>? ORDER BY epoch LIMIT 1"#, epoch.epoch)
                .fetch_optional(&mut self.transaction).await?;
            let upper_bound = successor.map_or(end, |line| line.inherited_through);
            if epoch.first <= inherited || epoch.last > upper_bound {
                return Err(ProjectStorageError::InvalidFeed);
            }
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
        let rows = sqlx_turso::query_file_as!(
            crate::project_feed_io::ProjectFeedRow,
            "src/project_feed_entry_batch.sql",
            positions
        )
        .fetch_all(&mut self.transaction)
        .await?;
        if rows.len() != keys.len() {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let mut entries = BTreeMap::new();
        for row in rows {
            let position = crate::project_record_codec::stored_counter(
                row.position.ok_or(ProjectStorageError::InvalidFeed)?,
                &self.project_id.clone().into(),
                "position",
            )?;
            let key = keys
                .iter()
                .find(|key| key.cursor.position == position)
                .cloned()
                .ok_or(ProjectStorageError::InvalidFeed)?;
            if entries
                .insert(
                    position,
                    StoredFeedEntry {
                        project: self.project_id.clone(),
                        key,
                        row,
                    },
                )
                .is_some()
            {
                return Err(ProjectStorageError::InvalidFeed);
            }
        }
        Ok(entries)
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
            .map(crate::project_record_codec::encode_merge_sweep)
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
                            row.time_basis,
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
                .fetch_all(&mut self.transaction)
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
                .fetch_all(&mut self.transaction)
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
        time: Option<String>,
        time_basis: String,
    ) -> Result<FeedPageKey, ProjectStorageError> {
        let invalid = || ProjectStorageError::InvalidFeed;
        let cursor = FeedCursor {
            epoch: EpochNumber::try_from(u64::try_from(epoch).map_err(|_| invalid())?)
                .map_err(|_| invalid())?,
            position: FeedPosition::try_from(u64::try_from(position).map_err(|_| invalid())?)
                .map_err(|_| invalid())?,
        };
        if cursor.position > self.high_water.position {
            return Err(invalid());
        }
        Ok(FeedPageKey {
            cursor,
            reference: RecordRef {
                kind: serde_json::from_value(serde_json::Value::String(kind))
                    .map_err(|_| invalid())?,
                id: RecordId::try_from(id).map_err(|_| invalid())?,
            },
            time: crate::project_record_codec::decode_record_time(
                time,
                &time_basis,
                &RecordRef {
                    kind: RecordKind::Project,
                    id: self.project_id.clone().into(),
                },
            )?,
        })
    }
}
