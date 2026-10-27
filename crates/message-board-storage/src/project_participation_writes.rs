//! Conversation intervals retain their join key as activity and closure advance.
use crate::project_participant_io::{invalid, role_name};
use crate::{ImportRowKind, ProjectStorageError, ProjectWriteTransaction};
use message_board::{
    Participant, ParticipantClosedReason, ParticipantNote, ParticipantProps, RecordId,
    RecordedActor, RecordedParticipantRole, ThreadId,
};

impl ProjectWriteTransaction {
    pub async fn put_participant(
        &mut self,
        thread: &ThreadId,
        participant: &Participant,
    ) -> Result<(), ProjectStorageError> {
        if matches!(participant.role, RecordedParticipantRole::NotRecorded)
            && (!matches!(
                self.admission,
                crate::project_import_keys::RecordAdmission::Import { .. }
            ) || participant.is_open())
        {
            return Err(invalid(thread, "participant.role"));
        }
        Participant::new(ParticipantProps {
            identity: participant.identity.clone(),
            role: participant.role,
            note: participant.note.clone(),
            joined_at_position: participant.joined_at_position,
            last_seen_position: participant.last_seen_position,
            closed_at_position: participant.closed_at_position,
            closed_reason: participant.closed_reason,
            replaced_by: participant.replaced_by.clone(),
        })
        .map_err(|_| invalid(thread, "participant"))?;
        let declared = RecordedActor::declared(participant.identity.clone());
        let actor = self.intern_actor(&declared).await?;
        let actor = actor.as_str();
        let thread_key = thread.as_str();
        let joined_epoch = i64::try_from(participant.joined_at_position.epoch.get())
            .map_err(|_| invalid(thread, "joined"))?;
        let joined_position = i64::try_from(participant.joined_at_position.position.get())
            .map_err(|_| invalid(thread, "joined"))?;
        let last_seen_epoch = i64::try_from(participant.last_seen_position.epoch.get())
            .map_err(|_| invalid(thread, "lastSeen"))?;
        let last_seen_position = i64::try_from(participant.last_seen_position.position.get())
            .map_err(|_| invalid(thread, "lastSeen"))?;
        let closed_epoch = participant
            .closed_at_position
            .map(|cursor| i64::try_from(cursor.epoch.get()))
            .transpose()
            .map_err(|_| invalid(thread, "closed"))?;
        let closed_position = participant
            .closed_at_position
            .map(|cursor| i64::try_from(cursor.position.get()))
            .transpose()
            .map_err(|_| invalid(thread, "closed"))?;
        let closed_reason = participant.closed_reason.map(|reason| match reason {
            ParticipantClosedReason::Left => "left",
            ParticipantClosedReason::Resolved => "resolved",
            ParticipantClosedReason::Replaced => "replaced",
            ParticipantClosedReason::Rejoined => "rejoined",
            ParticipantClosedReason::Promoted => "promoted",
        });
        let replaced_by = match &participant.replaced_by {
            None => None,
            Some(identity) => Some(
                self.intern_actor(&RecordedActor::declared(identity.clone()))
                    .await?,
            ),
        };
        let replaced_by = replaced_by.as_ref().map(RecordId::as_str);
        let role = role_name(participant.role);
        let note = participant.note.as_ref().map(ParticipantNote::as_str);
        let existing = sqlx_turso::query_scalar!(r#"SELECT participation_id AS "id!: String" FROM thread_participation WHERE actor_id=? AND thread_id=? AND joined_epoch=? AND joined_position=?"#, actor, thread_key, joined_epoch, joined_position).fetch_optional(&mut self.transaction).await?;
        let changed = match existing {
            None => {
                let canonical = serde_json::to_string(&declared)?;
                let epoch_key = joined_epoch.to_string();
                let position_key = joined_position.to_string();
                let key = self.admission.row_key(
                    ImportRowKind::ThreadParticipation,
                    &[&canonical, thread_key, &epoch_key, &position_key],
                )?;
                let key = key.as_str();
                sqlx_turso::query!("INSERT INTO thread_participation(participation_id, actor_id, thread_id, joined_epoch, joined_position, role, note, last_seen_epoch, last_seen_position, closed_epoch, closed_position, closed_reason, replaced_by) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)", key, actor, thread_key, joined_epoch, joined_position, role, note, last_seen_epoch, last_seen_position, closed_epoch, closed_position, closed_reason, replaced_by).execute(&mut self.transaction).await?
            }
            Some(key) => {
                let key =
                    RecordId::try_from(key).map_err(|_| invalid(thread, "participationId"))?;
                let key = key.as_str();
                sqlx_turso::query!("UPDATE thread_participation SET role=?, note=?, last_seen_epoch=?, last_seen_position=?, closed_epoch=?, closed_position=?, closed_reason=?, replaced_by=? WHERE participation_id=?", role, note, last_seen_epoch, last_seen_position, closed_epoch, closed_position, closed_reason, replaced_by, key).execute(&mut self.transaction).await?
            }
        };
        crate::project_record_codec::require_one_row(changed.rows_affected(), thread.clone().into())
    }
}
