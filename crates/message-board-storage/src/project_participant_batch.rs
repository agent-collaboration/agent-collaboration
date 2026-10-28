//! Participants and immutable anchors are fetched once per returned project page.
use crate::project_participant_io::{cursor, invalid, validate_participant_entry};
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{
    FeedCursor, Participant, ParticipantNote, ParticipantProps, ProjectId, RecordId, RecordRef,
    RecordedParticipantRole, ThreadId,
};
use sqlx::Executor;
use sqlx_turso::Turso;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct StoredParticipantRow {
    pub participation_id: String,
    pub thread_id: String,
    pub reader_actor: String,
    pub reader_identity: Option<String>,
    pub joined_epoch: i64,
    pub joined_position: i64,
    pub role: String,
    pub note: Option<String>,
    pub last_seen_epoch: i64,
    pub last_seen_position: i64,
    pub closed_epoch: Option<i64>,
    pub closed_position: Option<i64>,
    pub closed_reason: Option<String>,
    pub replaced_by: Option<String>,
    pub replacement_identity: Option<String>,
}
impl StoredParticipantRow {
    pub(super) fn decode(self, thread: &ThreadId) -> Result<Participant, ProjectStorageError> {
        tracing::trace!(target: "message_board_storage::decode", decode_kind = "participant");
        let reference: RecordRef = thread.clone().into();
        RecordId::try_from(self.participation_id)
            .map_err(|_| invalid(thread, "participationId"))?;
        if self.thread_id != thread.as_str() {
            return Err(invalid(thread, "thread"));
        }
        let identity = crate::project_actor_rows::decode_actor(
            &self.reader_actor,
            &self
                .reader_identity
                .ok_or_else(|| invalid(thread, "reader"))?,
        )?
        .declared_identity()
        .ok_or_else(|| invalid(thread, "reader"))?;
        let replaced_by = match (self.replaced_by, self.replacement_identity) {
            (None, None) => None,
            (Some(id), Some(identity)) => Some(
                crate::project_actor_rows::decode_actor(&id, &identity)?
                    .declared_identity()
                    .ok_or_else(|| invalid(thread, "replacedBy"))?,
            ),
            _ => return Err(invalid(thread, "replacedBy")),
        };
        let closed_at_position = match (self.closed_epoch, self.closed_position) {
            (None, None) => None,
            (Some(epoch), Some(position)) => Some(cursor(epoch, position, thread)?),
            _ => return Err(invalid(thread, "closed")),
        };
        Participant::new(ParticipantProps {
            identity,
            role: if self.role == "notRecorded" {
                RecordedParticipantRole::NotRecorded
            } else {
                RecordedParticipantRole::Recorded(crate::project_record_codec::stored_tag(
                    self.role, &reference, "role",
                )?)
            },
            note: self
                .note
                .map(|note| {
                    crate::project_record_codec::stored_text(
                        note,
                        &reference,
                        "note",
                        ParticipantNote::as_str,
                    )
                })
                .transpose()?,
            joined_at_position: cursor(self.joined_epoch, self.joined_position, thread)?,
            last_seen_position: cursor(self.last_seen_epoch, self.last_seen_position, thread)?,
            closed_at_position,
            closed_reason: self
                .closed_reason
                .map(|reason| {
                    crate::project_record_codec::stored_tag(reason, &reference, "closedReason")
                })
                .transpose()?,
            replaced_by,
        })
        .map_err(|_| invalid(thread, "participant"))
    }
}

pub struct ParticipantBatch {
    rows: BTreeMap<ThreadId, Vec<StoredParticipantRow>>,
}
impl ParticipantBatch {
    pub fn take(&mut self, thread: &ThreadId) -> Result<Vec<Participant>, ProjectStorageError> {
        self.rows
            .remove(thread)
            .unwrap_or_default()
            .into_iter()
            .map(|row| row.decode(thread))
            .collect()
    }
}

pub(super) async fn participant_batch<TScope>(
    transaction: &mut TScope,
    threads: &[ThreadId],
) -> Result<ParticipantBatch, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let keys = serde_json::to_string(&threads.iter().collect::<BTreeSet<_>>())?;
    let rows = sqlx_turso::query_file_as!(
        StoredParticipantRow,
        "src/project_participation_batch.sql",
        keys
    )
    .fetch_all(&mut *transaction)
    .await?;
    let mut grouped: BTreeMap<ThreadId, Vec<StoredParticipantRow>> = BTreeMap::new();
    for row in rows {
        let thread = ThreadId::try_from(row.thread_id.clone())
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        grouped.entry(thread).or_default().push(row);
    }
    Ok(ParticipantBatch { rows: grouped })
}

pub(super) async fn validate_participant_anchors<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    participants: &BTreeMap<ThreadId, Vec<Participant>>,
) -> Result<(), ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
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
    let rows = sqlx_turso::query_file_as!(
        crate::project_feed_io::ProjectFeedRow,
        "src/project_feed_entry_batch.sql",
        keys
    )
    .fetch_all(&mut *transaction)
    .await?;
    let mut anchors = BTreeMap::new();
    for row in rows {
        let entry = row
            .decode(project, true)?
            .ok_or(ProjectStorageError::InvalidFeed)?;
        if anchors.insert(entry.position, entry).is_some() {
            return Err(ProjectStorageError::InvalidFeed);
        }
    }
    for (thread, participants) in participants {
        for participant in participants {
            let cursors = [
                Some(participant.joined_at_position),
                Some(participant.last_seen_position),
                participant.closed_at_position,
            ]
            .into_iter()
            .flatten()
            .map(|cursor| (cursor.epoch, cursor.position))
            .collect::<BTreeSet<_>>();
            for (epoch, position) in cursors {
                let entry = anchors
                    .get(&position)
                    .ok_or_else(|| invalid(thread, "participant.position"))?;
                validate_participant_entry(project, thread, FeedCursor { epoch, position }, entry)?;
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
