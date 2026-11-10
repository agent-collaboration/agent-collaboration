//! Reconcile only the latest retained interval with the legacy current-row metadata.
use super::{
    Participant, ParticipantProps, PendingParticipant, ProjectBuilder, RecordedParticipantRole,
    UpgradeError, invalid,
};

impl ProjectBuilder<'_> {
    pub(super) fn finish_participants(&mut self) -> Result<(), UpgradeError> {
        for source in &self.source.participants {
            if !self.threads.contains_key(&source.root_id) {
                continue;
            }
            let identity = self.actor(&source.reader_key)?;
            let last_seen = self.cutoff(source.last_seen_activity)?;
            let closed = source
                .closed_at_activity
                .map(|old| self.cutoff(old))
                .transpose()?;
            let replaced_by = source
                .replaced_by
                .as_ref()
                .map(|key| self.actor(key))
                .transpose()?;
            let old_join = self.cutoff(source.joined_at_activity)?;
            let latest = self
                .rows
                .iter_mut()
                .rev()
                .flat_map(|row| row.participants.iter_mut().rev())
                .find(|(thread, participant)| {
                    thread == &source.root_id && participant.identity == identity
                })
                .ok_or_else(|| {
                    invalid(
                        "participation",
                        "current participant has no history interval",
                    )
                })?;
            let (_, participant) = latest;
            if matches!(participant.role, RecordedParticipantRole::Recorded(role) if role != source.role)
                || participant.closed_at_position != closed
                || participant.closed_reason != source.closed_reason
                || participant.replaced_by != replaced_by
                || old_join.position > participant.joined_at_position.position
            {
                return Err(invalid(
                    "participation",
                    "history and latest participant disagree",
                ));
            }
            participant.note = source.note.clone();
            participant.last_seen_position = last_seen;
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
            .map_err(|error| invalid("participation", error))?;
        }
        if self
            .participants
            .values()
            .any(|row| matches!(row, PendingParticipant::NotRecorded { .. }))
        {
            return Err(invalid(
                "participant_role",
                "open interval has no current recorded role",
            ));
        }
        Ok(())
    }
}
