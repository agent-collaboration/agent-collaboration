//! Participants and their immutable anchors are fetched once per project's returned page.
use crate::project_participant_io::{cursor, invalid, role_name, validate_participant_entry};
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{Identity, Participant, ProjectId, ThreadId};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct StoredParticipantRow {
    pub reader: String,
    pub joined_epoch: i64,
    pub joined_position: i64,
    pub role: String,
    pub active: i64,
    pub payload: String,
}
impl StoredParticipantRow {
    pub(super) fn decode(self, thread: &ThreadId) -> Result<Participant, ProjectStorageError> {
        tracing::trace!(target: "message_board_storage::decode", decode_kind = "participant");
        let participant: Participant =
            serde_json::from_str(&self.payload).map_err(|_| invalid(thread, "participant"))?;
        if participant.note.is_some() {
            crate::project_record_io::validate_canonical_project_text(
                &self.payload,
                &participant,
                &thread.clone().into(),
            )?;
        }
        let reader: Identity =
            serde_json::from_str(&self.reader).map_err(|_| invalid(thread, "reader"))?;
        let active = match self.active {
            0 => false,
            1 => true,
            _ => return Err(invalid(thread, "active")),
        };
        if participant.identity != reader
            || participant.joined_at_position
                != cursor(self.joined_epoch, self.joined_position, thread)?
            || participant.is_open() != active
            || role_name(participant.role) != self.role
        {
            return Err(invalid(thread, "participant"));
        }
        Ok(participant)
    }
}
pub struct ParticipantBatch {
    rows: BTreeMap<ThreadId, Vec<StoredParticipantRow>>,
}
impl ParticipantBatch {
    /// Body decoding is deferred until the thread is assembled into the response.
    pub fn take(&mut self, thread: &ThreadId) -> Result<Vec<Participant>, ProjectStorageError> {
        self.rows
            .remove(thread)
            .unwrap_or_default()
            .into_iter()
            .map(|row| row.decode(thread))
            .collect()
    }
}

pub(super) async fn participant_batch(
    connection: &mut sqlx_turso::TursoConnection,
    threads: &[ThreadId],
) -> Result<ParticipantBatch, ProjectStorageError> {
    let keys = serde_json::to_string(threads)?;
    // json_each.value has no text affinity. State the ID's type so the native planner
    // seeks participants_by_thread rather than scanning participants for each key.
    let rows = sqlx_turso::query!(r#"SELECT p.thread_id AS "thread_id!: String", p.reader AS "reader!: String", p.joined_epoch AS "joined_epoch!: i64", p.joined_position AS "joined_position!: i64", p.role AS "role!: String", p.active AS "active!: i64", p.payload AS "payload!: String" FROM json_each(?) k CROSS JOIN thread_participants p ON p.thread_id = CAST(k.value AS TEXT) ORDER BY p.thread_id, p.joined_position, p.reader"#, keys).fetch_all(connection).await?;
    let mut grouped: BTreeMap<ThreadId, Vec<StoredParticipantRow>> = BTreeMap::new();
    for row in rows {
        let thread =
            ThreadId::try_from(row.thread_id).map_err(|_| ProjectStorageError::InvalidFeed)?;
        grouped
            .entry(thread)
            .or_default()
            .push(StoredParticipantRow {
                reader: row.reader,
                joined_epoch: row.joined_epoch,
                joined_position: row.joined_position,
                role: row.role,
                active: row.active,
                payload: row.payload,
            });
    }
    Ok(ParticipantBatch { rows: grouped })
}

pub(super) async fn validate_participant_anchors(
    connection: &mut sqlx_turso::TursoConnection,
    project: &ProjectId,
    participants: &BTreeMap<ThreadId, Vec<Participant>>,
) -> Result<(), ProjectStorageError> {
    let positions: BTreeSet<_> = participants
        .values()
        .flatten()
        .flat_map(|participant| {
            [
                Some(participant.joined_at_position),
                Some(participant.last_seen_position),
                participant.closed_at_position,
            ]
        })
        .flatten()
        .map(|cursor| cursor.position.get())
        .collect();
    if positions.is_empty() {
        return Ok(());
    }
    let keys = serde_json::to_string(&positions)?;
    let rows = sqlx_turso::query!(r#"SELECT f.position AS "position!: i64", f.entry AS "entry!: String" FROM json_each(?) k CROSS JOIN project_feed f ON f.position = k.value ORDER BY f.position"#, keys).fetch_all(connection).await?;
    let anchors: BTreeMap<_, _> = rows
        .into_iter()
        .map(|row| (row.position, row.entry))
        .collect();
    for (thread, participants) in participants {
        for participant in participants {
            let mut cursors = Vec::new();
            for cursor in [
                Some(participant.joined_at_position),
                Some(participant.last_seen_position),
                participant.closed_at_position,
            ]
            .into_iter()
            .flatten()
            {
                if !cursors.contains(&cursor) {
                    cursors.push(cursor);
                }
            }
            for cursor in cursors {
                let position = i64::try_from(cursor.position.get())
                    .map_err(|_| invalid(thread, "participant.position"))?;
                let entry = anchors
                    .get(&position)
                    .ok_or_else(|| invalid(thread, "participant.position"))?;
                // Each participant is bound to its own thread even when anchors are shared.
                validate_participant_entry(project, thread, cursor, entry)?;
            }
        }
    }
    Ok(())
}
impl ProjectSnapshot {
    pub async fn participant_batch(
        &mut self,
        threads: &[ThreadId],
    ) -> Result<ParticipantBatch, ProjectStorageError> {
        participant_batch(&mut self.transaction, threads).await
    }
    pub async fn validate_participant_anchors(
        &mut self,
        participants: &BTreeMap<ThreadId, Vec<Participant>>,
    ) -> Result<(), ProjectStorageError> {
        validate_participant_anchors(&mut self.transaction, &self.project_id, participants).await
    }
}
impl ProjectWriteTransaction {
    pub async fn participant_batch(
        &mut self,
        threads: &[ThreadId],
    ) -> Result<ParticipantBatch, ProjectStorageError> {
        participant_batch(&mut self.transaction, threads).await
    }
    pub async fn validate_participant_anchors(
        &mut self,
        participants: &BTreeMap<ThreadId, Vec<Participant>>,
    ) -> Result<(), ProjectStorageError> {
        validate_participant_anchors(&mut self.transaction, &self.project_id, participants).await
    }
}
