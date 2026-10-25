//! One snapshot resolves this reader's stored cutoffs before inbox filtering compares positions.
use crate::{CursorValidity, ProjectSnapshot, ProjectStorageError};
use message_board::*;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InboxReaderPositions {
    pub actor: Option<RecordId>,
    pub main: Option<FeedCursor>,
    pub threads: BTreeMap<ThreadId, ThreadReaderPositions>,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ThreadReaderPositions {
    pub mark: Option<FeedCursor>,
    pub watch: Option<FeedCursor>,
}
struct StoredReaderPosition {
    kind: String,
    row_id: String,
    scope_kind: String,
    scope_id: String,
    epoch: i64,
    position: i64,
    active: Option<i64>,
}
impl ProjectSnapshot {
    pub(super) async fn inbox_reader_positions(
        &mut self,
        reader: &Identity,
    ) -> Result<InboxReaderPositions, ProjectStorageError> {
        let Some(actor) = crate::project_actor_rows::actor_id(
            &mut self.transaction,
            &RecordedActor::declared(reader.clone()),
        )
        .await?
        else {
            return Ok(InboxReaderPositions::default());
        };
        let key = actor.as_str();
        let rows=sqlx_turso::query_as!(StoredReaderPosition,r#"SELECT 'mark' AS "kind!: String",mark_id AS "row_id!: String",scope_kind AS "scope_kind!: String",scope_id AS "scope_id!: String",through_epoch AS "epoch!: i64",through_position AS "position!: i64",NULL AS "active?: i64" FROM read_marks WHERE reader_actor=? UNION ALL SELECT 'watch' AS "kind!: String",watch_id AS "row_id!: String",'thread' AS "scope_kind!: String",thread_id AS "scope_id!: String",start_epoch AS "epoch!: i64",start_position AS "position!: i64",active AS "active?: i64" FROM thread_watches WHERE reader_actor=?"#,key,key).fetch_all(&mut self.transaction).await?;
        let mut parsed = Vec::with_capacity(rows.len());
        let mut cursors = Vec::new();
        for row in rows {
            let reference = RecordRef {
                kind: RecordKind::ReadMark,
                id: RecordId::try_from(row.row_id.clone())
                    .map_err(|_| ProjectStorageError::InvalidFeed)?,
            };
            let cursor = FeedCursor {
                epoch: crate::project_record_codec::stored_counter(row.epoch, &reference, "epoch")?,
                position: crate::project_record_codec::stored_counter(
                    row.position,
                    &reference,
                    "position",
                )?,
            };
            let active = match (row.kind.as_str(), row.active) {
                ("mark", None) => true,
                ("watch", Some(active)) => {
                    crate::project_record_codec::stored_bool(active, &reference, "watch.active")?
                }
                _ => return Err(ProjectStorageError::InvalidFeed),
            };
            match row.scope_kind.as_str() {
                "project" if row.scope_id == self.project_id.as_str() => {}
                "thread" => {
                    ThreadId::try_from(row.scope_id.clone())
                        .map_err(|_| ProjectStorageError::InvalidFeed)?;
                }
                _ => return Err(ProjectStorageError::InvalidFeed),
            }
            if active {
                cursors.push(cursor);
            }
            parsed.push((row, reference, cursor, active));
        }
        let validities = crate::project_epoch_reads::cursor_validities(
            &mut self.transaction,
            &cursors,
            self.high_water,
        )
        .await?;
        let mut positions = InboxReaderPositions {
            actor: Some(actor),
            ..Default::default()
        };
        for (row, reference, cursor, active) in parsed {
            let resolved = if active {
                match validities
                    .get(&(cursor.epoch, cursor.position))
                    .ok_or(ProjectStorageError::InvalidFeed)?
                {
                    CursorValidity::Valid => Some(cursor),
                    CursorValidity::Superseded { resume_from } if row.kind == "mark" => {
                        Some(*resume_from)
                    }
                    CursorValidity::Superseded { resume_from } => {
                        return Err(ProjectStorageError::CursorSuperseded {
                            resume_from: *resume_from,
                        });
                    }
                    CursorValidity::Invalid => {
                        return Err(ProjectStorageError::InvalidRecord {
                            record: reference,
                            field: if row.kind == "watch" {
                                "watch.start"
                            } else {
                                "through"
                            },
                        });
                    }
                }
            } else {
                None
            };
            match (row.kind.as_str(), row.scope_kind.as_str()) {
                ("mark", "project") => positions.main = resolved,
                (kind, "thread") => {
                    let thread = ThreadId::try_from(row.scope_id)
                        .map_err(|_| ProjectStorageError::InvalidFeed)?;
                    let state = positions.threads.entry(thread).or_default();
                    match kind {
                        "mark" => state.mark = resolved,
                        "watch" => {
                            state.watch = resolved;
                        }
                        _ => return Err(ProjectStorageError::InvalidFeed),
                    }
                }
                _ => return Err(ProjectStorageError::InvalidFeed),
            }
        }
        Ok(positions)
    }
}
