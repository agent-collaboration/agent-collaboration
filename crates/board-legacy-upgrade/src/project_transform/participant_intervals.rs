//! Unknown roles remain drafts until closure; they never become open Participants.
use super::{
    ActivityKind, FeedCursor, Identity, ImplementerHolder, LegacyActivity, OrchestratorHolder,
    Participant, ParticipantClosedReason, ParticipantNote, ParticipantProps, ParticipantRole,
    ProjectBuilder, RecordedParticipantRole, Thread, ThreadId, UpgradeError, invalid,
};

#[derive(Clone)]
pub(super) enum PendingParticipant {
    Recorded(Participant),
    NotRecorded {
        identity: Identity,
        joined: FeedCursor,
        note: Option<ParticipantNote>,
    },
}

impl PendingParticipant {
    fn note(&self) -> Option<ParticipantNote> {
        match self {
            Self::Recorded(row) => row.note.clone(),
            Self::NotRecorded { note, .. } => note.clone(),
        }
    }

    pub(super) fn observe_post(&mut self, at: FeedCursor) -> Option<Participant> {
        match self {
            Self::Recorded(row) => {
                row.last_seen_position = at;
                Some(row.clone())
            }
            Self::NotRecorded { .. } => None,
        }
    }
}

impl ProjectBuilder<'_> {
    pub(super) fn role_for_join(
        &self,
        activity: &LegacyActivity,
    ) -> Result<Option<ParticipantRole>, UpgradeError> {
        if let Some(role) = activity.participant_role {
            return Ok(Some(role));
        }
        let key = activity
            .participant_key
            .as_ref()
            .ok_or_else(|| invalid("participant_key", "join missing actor"))?;
        let later_closure = self.source.activity.iter().any(|later| {
            later.root_id == activity.root_id
                && later.activity_sequence > activity.activity_sequence
                && match later.kind {
                    ActivityKind::ThreadResolved => true,
                    ActivityKind::ParticipantJoined | ActivityKind::ParticipantLeft => {
                        later.participant_key.as_ref() == Some(key)
                    }
                    ActivityKind::OrchestratorReplaced | ActivityKind::ImplementerReplaced => {
                        later.participant_key.as_ref() == Some(key)
                            || later.replaced_participant_key.as_ref() == Some(key)
                    }
                    ActivityKind::MainMessageCreated
                    | ActivityKind::ThreadMessageCreated
                    | ActivityKind::ThreadUnresolved => false,
                }
        });
        if later_closure {
            return Ok(None);
        }
        self.source
            .participants
            .iter()
            .find(|row| {
                row.reader_key == *key
                    && Some(&row.root_id) == activity.root_id.as_ref()
                    && row.closed_at_activity.is_none()
            })
            .map(|row| Some(row.role))
            .ok_or_else(|| {
                invalid(
                    "participant_role",
                    "open interval has no current recorded role",
                )
            })
    }

    pub(super) fn close_interval(
        &mut self,
        thread: &ThreadId,
        actor: &Identity,
        at: FeedCursor,
        reason: ParticipantClosedReason,
        replaced_by: Option<Identity>,
    ) -> Result<Participant, UpgradeError> {
        let pending = self
            .participants
            .remove(&(thread.clone(), actor.clone()))
            .ok_or_else(|| invalid("participation", "closing absent interval"))?;
        let (identity, role, joined, note) = match pending {
            PendingParticipant::Recorded(row) => {
                (row.identity, row.role, row.joined_at_position, row.note)
            }
            PendingParticipant::NotRecorded {
                identity,
                joined,
                note,
                ..
            } => (identity, RecordedParticipantRole::NotRecorded, joined, note),
        };
        Participant::new(ParticipantProps {
            identity,
            role,
            note,
            joined_at_position: joined,
            last_seen_position: at,
            closed_at_position: Some(at),
            closed_reason: Some(reason),
            replaced_by,
        })
        .map_err(|error| invalid("participation", error))
    }

    pub(super) fn join(
        &mut self,
        thread: &ThreadId,
        actor: &Identity,
        role: Option<ParticipantRole>,
        at: FeedCursor,
        note: Option<ParticipantNote>,
        closure: ParticipantClosedReason,
    ) -> Result<Vec<(ThreadId, Participant)>, UpgradeError> {
        let mut changes = Vec::new();
        let previous = self
            .participants
            .get(&(thread.clone(), actor.clone()))
            .cloned();
        if previous.is_some() {
            changes.push((
                thread.clone(),
                self.close_interval(thread, actor, at, closure, None)?,
            ));
        }
        let note = note.or_else(|| previous.and_then(|row| row.note()));
        let pending = match role {
            Some(role) => {
                let participant = Participant::new(ParticipantProps {
                    identity: actor.clone(),
                    role: RecordedParticipantRole::Recorded(role),
                    note,
                    joined_at_position: at,
                    last_seen_position: at,
                    closed_at_position: None,
                    closed_reason: None,
                    replaced_by: None,
                })
                .map_err(|error| invalid("participation", error))?;
                changes.push((thread.clone(), participant.clone()));
                PendingParticipant::Recorded(participant)
            }
            None => PendingParticipant::NotRecorded {
                identity: actor.clone(),
                joined: at,
                note,
            },
        };
        self.participants
            .insert((thread.clone(), actor.clone()), pending);
        Ok(changes)
    }

    pub(super) fn update_holders(&self, thread: &mut Thread) {
        thread.orchestrator = None;
        thread.implementer = None;
        for ((id, _), pending) in &self.participants {
            if id != &thread.thread_id {
                continue;
            }
            let PendingParticipant::Recorded(row) = pending else {
                continue;
            };
            match row.role {
                RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator) => {
                    thread.orchestrator = Some(OrchestratorHolder {
                        identity: row.identity.clone(),
                        last_seen_position: row.last_seen_position,
                    });
                }
                RecordedParticipantRole::Recorded(ParticipantRole::Implementer) => {
                    thread.implementer = Some(ImplementerHolder {
                        identity: row.identity.clone(),
                        last_seen_position: row.last_seen_position,
                    });
                }
                RecordedParticipantRole::Recorded(
                    ParticipantRole::Advisor
                    | ParticipantRole::Reviewer
                    | ParticipantRole::Participant,
                )
                | RecordedParticipantRole::NotRecorded => {}
            }
        }
    }
}
