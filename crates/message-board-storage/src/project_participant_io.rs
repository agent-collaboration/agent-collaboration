//! Native conversation intervals and interest, independent of authority assignments.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{
    EpochNumber, FeedContent, FeedCursor, FeedEntry, FeedPosition, Participant, ParticipantRole,
    ProjectId, Record, RecordedParticipantRole, ThreadId,
};

pub(super) fn invalid(thread: &ThreadId, field: &'static str) -> ProjectStorageError {
    ProjectStorageError::InvalidRecord {
        record: thread.clone().into(),
        field,
    }
}
pub(super) fn role_name(role: RecordedParticipantRole) -> &'static str {
    match role {
        RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator) => "orchestrator",
        RecordedParticipantRole::Recorded(ParticipantRole::Implementer) => "implementer",
        RecordedParticipantRole::Recorded(ParticipantRole::Advisor) => "advisor",
        RecordedParticipantRole::Recorded(ParticipantRole::Reviewer) => "reviewer",
        RecordedParticipantRole::Recorded(ParticipantRole::Participant) => "participant",
        RecordedParticipantRole::NotRecorded => "notRecorded",
    }
}
pub(super) fn cursor(
    epoch: i64,
    position: i64,
    thread: &ThreadId,
) -> Result<FeedCursor, ProjectStorageError> {
    Ok(FeedCursor {
        epoch: EpochNumber::try_from(u64::try_from(epoch).map_err(|_| invalid(thread, "epoch"))?)
            .map_err(|_| invalid(thread, "epoch"))?,
        position: FeedPosition::try_from(
            u64::try_from(position).map_err(|_| invalid(thread, "position"))?,
        )
        .map_err(|_| invalid(thread, "position"))?,
    })
}
async fn participants<TScope>(
    connection: &mut TScope,
    project: &ProjectId,
    thread: &ThreadId,
) -> Result<Vec<Participant>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: sqlx::Executor<'scope, Database = sqlx_turso::Turso>,
{
    let mut batch = super::project_participant_batch::participant_batch(
        connection,
        std::slice::from_ref(thread),
    )
    .await?;
    let participants = batch.take(thread)?;
    let by_thread = std::collections::BTreeMap::from([(thread.clone(), participants)]);
    super::project_participant_batch::validate_participant_anchors(connection, project, &by_thread)
        .await?;
    Ok(by_thread.into_values().next().unwrap_or_default())
}
pub(super) fn validate_participant_entry(
    project: &ProjectId,
    thread: &ThreadId,
    cursor: FeedCursor,
    entry: &FeedEntry,
) -> Result<(), ProjectStorageError> {
    let belongs = entry.epoch == cursor.epoch
        && entry.position == cursor.position
        && match &entry.content {
            FeedContent::Shown(record)
                if record.reference() == entry.record
                    && record.envelope().position.project_cursor() == Some(cursor)
                    && record.envelope().scope.project.as_ref() == Some(project) =>
            {
                match record.as_ref() {
                    Record::Thread(row) => row.thread_id == *thread,
                    Record::Post(row) => {
                        row.post_id.as_str() == thread.as_str()
                            || matches!(&row.placement,message_board::Placement::Thread {thread_id} if thread_id == thread)
                    }
                    _ => false,
                }
            }
            FeedContent::Shown(_) | FeedContent::Redacted => false,
        };
    if !belongs {
        return Err(invalid(thread, "participant.position"));
    }
    Ok(())
}
impl ProjectSnapshot {
    pub async fn participants(
        &mut self,
        thread: &ThreadId,
    ) -> Result<Vec<Participant>, ProjectStorageError> {
        participants(&mut self.transaction, &self.project_id, thread).await
    }
}
impl ProjectWriteTransaction {
    pub async fn participants(
        &mut self,
        thread: &ThreadId,
    ) -> Result<Vec<Participant>, ProjectStorageError> {
        participants(&mut self.transaction, &self.project_id, thread).await
    }
}
