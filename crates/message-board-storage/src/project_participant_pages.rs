//! Interval pagination selects typed metadata before decoding only returned participants.
use crate::project_participant_batch::StoredParticipantRow;
use crate::project_participant_io::{cursor, invalid};
use crate::{ProjectSnapshot, ProjectStorageError};
use message_board::{FeedCursor, Identity, Participant, RecordedActor, ThreadId};
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
        actor_id: &str,
        identity: Option<&str>,
        epoch: i64,
        position: i64,
    ) -> Result<Self, ProjectStorageError> {
        let reader = crate::project_actor_rows::decode_actor(
            actor_id,
            identity.ok_or_else(|| invalid(thread, "reader"))?,
        )?
        .declared_identity()
        .ok_or_else(|| invalid(thread, "reader"))?;
        Ok(Self {
            reader,
            joined: cursor(epoch, position, thread)?,
        })
    }
}

pub struct ParticipantIntervalBatch {
    thread: ThreadId,
    rows: Vec<(ParticipantIntervalKey, StoredParticipantRow)>,
}
impl ParticipantIntervalBatch {
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
        let after_reader = after
            .map(|key| serde_json::to_string(&RecordedActor::declared(key.reader.clone())))
            .transpose()?;
        let after_epoch = after
            .map(|key| i64::try_from(key.joined.epoch.get()))
            .transpose()
            .map_err(|_| invalid(thread, "epoch"))?;
        let after_position = after
            .map(|key| i64::try_from(key.joined.position.get()))
            .transpose()
            .map_err(|_| invalid(thread, "position"))?;
        let rows = sqlx_turso::query!(r#"SELECT p.actor_id AS "reader_actor!: String", a.canonical_identity AS "reader_identity?: String", p.joined_epoch AS "joined_epoch!: i64", p.joined_position AS "joined_position!: i64" FROM thread_participation p LEFT JOIN actors a ON a.actor_id=p.actor_id WHERE p.thread_id=? AND (? IS NULL OR (a.canonical_identity,p.joined_epoch,p.joined_position)>(?,?,?)) ORDER BY a.canonical_identity,p.joined_epoch,p.joined_position LIMIT ?"#, id, after_reader, after_reader, after_epoch, after_position, limit).fetch_all(&mut self.transaction).await?;
        rows.into_iter()
            .map(|row| {
                ParticipantIntervalKey::stored(
                    thread,
                    &row.reader_actor,
                    row.reader_identity.as_deref(),
                    row.joined_epoch,
                    row.joined_position,
                )
            })
            .collect()
    }
    pub async fn participant_holder_keys(
        &mut self,
        thread: &ThreadId,
    ) -> Result<Vec<ParticipantIntervalKey>, ProjectStorageError> {
        let id = thread.as_str();
        let rows = sqlx_turso::query!(r#"SELECT p.actor_id AS "reader_actor!: String", a.canonical_identity AS "reader_identity?: String", p.joined_epoch AS "joined_epoch!: i64", p.joined_position AS "joined_position!: i64", p.role AS "role!: String" FROM thread_participation p LEFT JOIN actors a ON a.actor_id=p.actor_id WHERE p.thread_id=? AND p.closed_epoch IS NULL AND p.role NOT IN ('advisor','reviewer','participant') ORDER BY p.role LIMIT 3"#, id).fetch_all(&mut self.transaction).await?;
        let mut roles = std::collections::BTreeSet::new();
        let mut keys = Vec::new();
        for row in rows {
            let role: message_board::ParticipantRole = crate::project_record_codec::stored_tag(
                row.role.clone(),
                &thread.clone().into(),
                "role",
            )?;
            if !matches!(
                role,
                message_board::ParticipantRole::Orchestrator
                    | message_board::ParticipantRole::Implementer
            ) {
                return Err(invalid(thread, "participant.role"));
            }
            if !roles.insert(row.role) {
                return Err(invalid(thread, "participant.role"));
            }
            keys.push(ParticipantIntervalKey::stored(
                thread,
                &row.reader_actor,
                row.reader_identity.as_deref(),
                row.joined_epoch,
                row.joined_position,
            )?);
        }
        Ok(keys)
    }
    pub async fn participant_interval_batch(
        &mut self,
        thread: &ThreadId,
        keys: &[ParticipantIntervalKey],
    ) -> Result<ParticipantIntervalBatch, ProjectStorageError> {
        let mut encoded = Vec::with_capacity(keys.len());
        for key in keys {
            encoded.push((
                serde_json::to_string(&RecordedActor::declared(key.reader.clone()))?,
                key.joined.epoch.get(),
                key.joined.position.get(),
            ));
        }
        let keys = serde_json::to_string(&encoded)?;
        let id = thread.as_str();
        let rows = sqlx_turso::query_file_as!(
            StoredParticipantRow,
            "src/project_participation_interval_batch.sql",
            keys,
            id
        )
        .fetch_all(&mut self.transaction)
        .await?;
        let mut selected = Vec::with_capacity(rows.len());
        for row in rows {
            let key = ParticipantIntervalKey::stored(
                thread,
                &row.reader_actor,
                row.reader_identity.as_deref(),
                row.joined_epoch,
                row.joined_position,
            )?;
            selected.push((key, row));
        }
        Ok(ParticipantIntervalBatch {
            thread: thread.clone(),
            rows: selected,
        })
    }
}
