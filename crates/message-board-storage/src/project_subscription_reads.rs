//! A subscription window reads post metadata, then batches only eligible watch operands.
use crate::project_participant_io::{cursor, invalid};
use crate::{FeedPageBounds, FeedPageDirection, FeedPageKey, ProjectSnapshot, ProjectStorageError};
use message_board::*;
use std::collections::BTreeMap;

pub struct PostActivityKey {
    pub feed: FeedPageKey,
    pub thread: ThreadId,
    pub actor: Identity,
    pub placement: Placement,
}
impl ProjectSnapshot {
    pub async fn post_activity_keys(
        &mut self,
        bounds: &FeedPageBounds,
        thread: Option<&ThreadId>,
    ) -> Result<Vec<PostActivityKey>, ProjectStorageError> {
        if !matches!(bounds.direction, FeedPageDirection::Forward) || bounds.sweep.is_some() {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let start =
            i64::try_from(bounds.after.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let end =
            i64::try_from(bounds.through.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let limit = i64::try_from(bounds.limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let thread = thread.map(ThreadId::as_str);
        let rows = sqlx_turso::query!(r#"SELECT f.position AS "position!: i64", f.epoch AS "epoch!: i64", f.record_id AS "record_id!: String", f.router_time AS "time?: String", f.time_basis AS "time_basis!: String", p.thread_id AS "thread_id!: String", json_quote(json_extract(f.content, '$.record.record.placement')) AS "placement!: String", json_quote(json_extract(f.content, '$.record.record.actor')) AS "actor!: String" FROM project_feed f INDEXED BY feed_by_kind_position LEFT JOIN posts p ON p.post_id = f.record_id WHERE f.record_kind = 'post' AND f.position > ? AND f.position <= ? AND (? IS NULL OR p.thread_id = ? OR p.post_id IS NULL) ORDER BY f.position LIMIT ?"#, start, end, thread, thread, limit).fetch_all(&mut self.transaction).await?;
        rows.into_iter()
            .map(|row| {
                let feed = self.feed_page_key(
                    row.position,
                    row.epoch,
                    "post".to_owned(),
                    row.record_id,
                    row.time,
                    row.time_basis,
                )?;
                let placement: Placement = serde_json::from_str(&row.placement).map_err(|_| {
                    ProjectStorageError::InvalidRecord {
                        record: feed.reference.clone(),
                        field: "placement",
                    }
                })?;
                let thread = match &placement {
                    Placement::Thread { thread_id } => thread_id.clone(),
                    Placement::Project { project_id } if project_id == &self.project_id => {
                        ThreadId::try_from(feed.reference.id.as_str().to_owned())
                            .map_err(|_| ProjectStorageError::InvalidFeed)?
                    }
                    Placement::Project { .. } => {
                        return Err(ProjectStorageError::InvalidRecord {
                            record: feed.reference,
                            field: "placement",
                        });
                    }
                };
                let stored_thread = ThreadId::try_from(row.thread_id)
                    .map_err(|_| ProjectStorageError::InvalidFeed)?;
                if stored_thread != thread {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: feed.reference,
                        field: "placement",
                    });
                }
                let actor = serde_json::from_str(&row.actor).map_err(|_| {
                    ProjectStorageError::InvalidRecord {
                        record: feed.reference.clone(),
                        field: "actor",
                    }
                })?;
                Ok(PostActivityKey {
                    feed,
                    thread,
                    actor,
                    placement,
                })
            })
            .collect()
    }
    pub async fn effective_thread_watch_batch(
        &mut self,
        threads: &[ThreadId],
        reader: &Identity,
    ) -> Result<BTreeMap<ThreadId, Option<FeedCursor>>, ProjectStorageError> {
        if threads.is_empty() {
            return Ok(BTreeMap::new());
        }
        let keys = serde_json::to_string(threads)?;
        let Some(reader_actor) = crate::project_actor_rows::actor_id(
            &mut self.transaction,
            &RecordedActor::declared(reader.clone()),
        )
        .await?
        else {
            return Ok(threads
                .iter()
                .cloned()
                .map(|thread| (thread, None))
                .collect());
        };
        let reader_key = reader_actor.as_str();
        let rows = sqlx_turso::query!(r#"SELECT CAST(k.value AS TEXT) AS "thread_id!: String", w.start_epoch AS "epoch: i64", w.start_position AS "position: i64", w.active AS "active: i64", w.watch_id AS "watch_id?: String", f.epoch AS "assigning_epoch?: i64" FROM json_each(?) k LEFT JOIN thread_watches w ON w.reader_actor = ? AND w.thread_id = CAST(k.value AS TEXT) LEFT JOIN project_feed f ON f.position=w.start_position"#, keys, reader_key).fetch_all(&mut self.transaction).await?;
        let mut watches = BTreeMap::new();
        for row in rows {
            let thread =
                ThreadId::try_from(row.thread_id).map_err(|_| ProjectStorageError::InvalidFeed)?;
            let watch = match (row.epoch, row.position, row.active) {
                (None, None, None) => None,
                (Some(epoch), Some(position), Some(active)) => {
                    let boundary = cursor(epoch, position, &thread)?;
                    RecordId::try_from(row.watch_id.ok_or_else(|| invalid(&thread, "watchId"))?)
                        .map_err(|_| invalid(&thread, "watchId"))?;
                    if boundary.position != FeedPosition::ZERO
                        && row
                            .assigning_epoch
                            .and_then(|epoch| u64::try_from(epoch).ok())
                            != Some(boundary.epoch.get())
                    {
                        crate::project_epoch_reads::require_valid_cursor(
                            &mut self.transaction,
                            boundary,
                            self.high_water,
                        )
                        .await?;
                    }
                    match active {
                        0 => None,
                        1 => Some(boundary),
                        _ => return Err(invalid(&thread, "active")),
                    }
                }
                _ => return Err(invalid(&thread, "threadWatch")),
            };
            watches.insert(thread, watch);
        }
        Ok(watches)
    }
}
