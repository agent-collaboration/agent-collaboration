//! Indexed interval keys precede opaque body reads and page-dependent anchor validation.
use crate::project_participant_batch::StoredParticipantRow;
use crate::project_participant_io::{cursor, invalid};
use crate::{ProjectSnapshot, ProjectStorageError};
use message_board::{FeedCursor, Identity, Participant, ThreadId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ParticipantIntervalKey {
    pub reader: Identity,
    pub joined: FeedCursor,
}
impl ParticipantIntervalKey {
    pub fn of(participant: &Participant) -> Self {
        Self {
            reader: participant.identity.clone(),
            joined: participant.joined_at_position,
        }
    }
    fn stored(
        thread: &ThreadId,
        reader: String,
        epoch: i64,
        position: i64,
    ) -> Result<Self, ProjectStorageError> {
        let identity: Identity =
            serde_json::from_str(&reader).map_err(|_| invalid(thread, "reader"))?;
        if serde_json::to_string(&identity)? != reader {
            return Err(invalid(thread, "reader"));
        }
        Ok(Self {
            reader: identity,
            joined: cursor(epoch, position, thread)?,
        })
    }
}

pub struct ParticipantIntervalBatch {
    thread: ThreadId,
    rows: Vec<(ParticipantIntervalKey, StoredParticipantRow)>,
}
impl ParticipantIntervalBatch {
    /// Decode only an interval the response or its current-holder header needs.
    pub fn take(
        &mut self,
        key: &ParticipantIntervalKey,
    ) -> Result<Participant, ProjectStorageError> {
        let index = self
            .rows
            .iter()
            .position(|(stored, _)| stored == key)
            .ok_or_else(|| invalid(&self.thread, "participant"))?;
        self.rows.swap_remove(index).1.decode(&self.thread)
    }
}

impl ProjectSnapshot {
    pub async fn participant_page_keys(
        &mut self,
        thread: &ThreadId,
        after: Option<&ParticipantIntervalKey>,
        limit: u64,
    ) -> Result<Vec<ParticipantIntervalKey>, ProjectStorageError> {
        let id = thread.as_str();
        let limit = i64::try_from(limit).map_err(|_| invalid(thread, "limit"))?;
        if let Some(after) = after {
            let reader = serde_json::to_string(&after.reader)?;
            let epoch =
                i64::try_from(after.joined.epoch.get()).map_err(|_| invalid(thread, "epoch"))?;
            let position = i64::try_from(after.joined.position.get())
                .map_err(|_| invalid(thread, "position"))?;
            return sqlx_turso::query!(r#"SELECT reader AS "reader!: String", joined_epoch AS "joined_epoch!: i64", joined_position AS "joined_position!: i64" FROM thread_participants WHERE thread_id = ? AND (reader, joined_epoch, joined_position) > (?, ?, ?) ORDER BY reader, joined_epoch, joined_position LIMIT ?"#,id,reader,epoch,position,limit)
                .fetch_all(&mut *self.transaction).await?.into_iter().map(|row| ParticipantIntervalKey::stored(thread,row.reader,row.joined_epoch,row.joined_position)).collect();
        }
        sqlx_turso::query!(r#"SELECT reader AS "reader!: String", joined_epoch AS "joined_epoch!: i64", joined_position AS "joined_position!: i64" FROM thread_participants WHERE thread_id = ? ORDER BY reader, joined_epoch, joined_position LIMIT ?"#,id,limit)
            .fetch_all(&mut *self.transaction).await?.into_iter().map(|row| ParticipantIntervalKey::stored(thread,row.reader,row.joined_epoch,row.joined_position)).collect()
    }
    pub async fn participant_holder_keys(
        &mut self,
        thread: &ThreadId,
    ) -> Result<Vec<ParticipantIntervalKey>, ProjectStorageError> {
        let id = thread.as_str();
        let keys = sqlx_turso::query!(r#"SELECT reader AS "reader!: String", joined_epoch AS "joined_epoch!: i64", joined_position AS "joined_position!: i64" FROM thread_participants INDEXED BY one_unique_thread_seat WHERE thread_id = ? AND active = 1 AND role IN ('orchestrator', 'implementer') LIMIT 3"#,id)
            .fetch_all(&mut *self.transaction).await?.into_iter().map(|row| ParticipantIntervalKey::stored(thread,row.reader,row.joined_epoch,row.joined_position)).collect::<Result<Vec<_>,_>>()?;
        if keys.len() > 2 {
            return Err(invalid(thread, "participant.role"));
        }
        Ok(keys)
    }
    pub async fn participant_interval_batch(
        &mut self,
        thread: &ThreadId,
        keys: &[ParticipantIntervalKey],
    ) -> Result<ParticipantIntervalBatch, ProjectStorageError> {
        let mut stored = Vec::new();
        for key in keys {
            stored.push((
                serde_json::to_string(&key.reader)?,
                i64::try_from(key.joined.epoch.get()).map_err(|_| invalid(thread, "epoch"))?,
                i64::try_from(key.joined.position.get())
                    .map_err(|_| invalid(thread, "position"))?,
            ));
        }
        let keys = serde_json::to_string(&stored)?;
        let id = thread.as_str();
        let rows = sqlx_turso::query!(r#"SELECT p.reader AS "reader!: String", p.joined_epoch AS "joined_epoch!: i64", p.joined_position AS "joined_position!: i64", p.role AS "role!: String", p.active AS "active!: i64", p.payload AS "payload!: String" FROM json_each(?) k CROSS JOIN thread_participants p ON p.reader = CAST(json_extract(k.value, '$[0]') AS TEXT) AND p.thread_id = ? AND p.joined_epoch = json_extract(k.value, '$[1]') AND p.joined_position = json_extract(k.value, '$[2]')"#,keys,id).fetch_all(&mut *self.transaction).await?;
        let mut selected = Vec::new();
        for row in rows {
            let key = ParticipantIntervalKey::stored(
                thread,
                row.reader.clone(),
                row.joined_epoch,
                row.joined_position,
            )?;
            selected.push((
                key,
                StoredParticipantRow {
                    reader: row.reader,
                    joined_epoch: row.joined_epoch,
                    joined_position: row.joined_position,
                    role: row.role,
                    active: row.active,
                    payload: row.payload,
                },
            ));
        }
        Ok(ParticipantIntervalBatch {
            thread: thread.clone(),
            rows: selected,
        })
    }
}
