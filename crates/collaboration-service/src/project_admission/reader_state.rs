//! Mutable caller state is read after every decision, including ledger replay.
use super::{ProjectAdmission, ProjectAdmissionError};
use message_board::{
    BoardOperation, FeedCursor, FeedPosition, Identity, ProjectId, ProjectRequest, ReadScope,
    ReaderState, RecordKind, RecordRef, RequestResult, RoleSubject, ThreadId, TrailSubject,
};
use message_board_storage::{ProjectSnapshot, StoreSchemaState};

impl ProjectAdmission {
    pub(super) async fn reader_after_result(
        &self,
        request: &ProjectRequest,
        result: &RequestResult,
    ) -> Result<Option<ReaderState>, ProjectAdmissionError> {
        if matches!(result, RequestResult::SchemaNotReady(_)) {
            return Ok(None);
        }
        match self.reader_state(request).await {
            // Losing the readable copy must not replace its native write
            // refusal with a failure from this optional companion lookup.
            Err(ProjectAdmissionError::Registry(
                crate::ProjectRegistryError::Unavailable { .. }
                | crate::ProjectRegistryError::Installing { .. }
                | crate::ProjectRegistryError::Quarantined { .. },
            )) if matches!(result, RequestResult::NotWriter { .. }) => Ok(None),
            outcome => outcome,
        }
    }

    pub(super) async fn reader_state(
        &self,
        request: &ProjectRequest,
    ) -> Result<Option<ReaderState>, ProjectAdmissionError> {
        let Some(store) = self.registry.read_project(&request.project_id).await? else {
            return Ok(None);
        };
        if !matches!(store.schema_state().await?, StoreSchemaState::Ready { .. }) {
            return Ok(None);
        }
        let mut snapshot = store.snapshot().await?;
        let subject = reader_subject(request, &mut snapshot).await?;
        reader_snapshot(&request.project_id, &request.actor, subject, snapshot)
            .await
            .map(Some)
    }

    pub(super) async fn project_reader_state(
        &self,
        project: &ProjectId,
        reader: &Identity,
    ) -> Result<Option<ReaderState>, ProjectAdmissionError> {
        let Some(store) = self.registry.read_project(project).await? else {
            return Ok(None);
        };
        if !matches!(store.schema_state().await?, StoreSchemaState::Ready { .. }) {
            return Ok(None);
        }
        reader_snapshot(
            project,
            reader,
            project.clone().into(),
            store.snapshot().await?,
        )
        .await
        .map(Some)
    }
}

async fn reader_snapshot(
    project: &ProjectId,
    reader: &Identity,
    subject: RecordRef,
    mut snapshot: crate::ProjectReadSnapshot,
) -> Result<ReaderState, ProjectAdmissionError> {
    let scope = if subject.kind == RecordKind::Thread {
        ReadScope::Thread {
            thread_id: ThreadId::try_from(subject.id.as_str().to_owned())
                .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?,
        }
    } else {
        ReadScope::Project {
            project_id: project.clone(),
        }
    };
    let high_water = snapshot.high_water();
    let read_through = snapshot.read_mark(reader, &scope).await?.map_or(
        FeedCursor {
            epoch: high_water.epoch,
            position: FeedPosition::ZERO,
        },
        |mark| mark.through,
    );
    let reader_state = snapshot.unread_state_after(&subject, read_through).await?;
    snapshot.finish().await?;
    Ok(reader_state)
}

async fn reader_subject(
    request: &ProjectRequest,
    read: &mut ProjectSnapshot,
) -> Result<RecordRef, ProjectAdmissionError> {
    Ok(match &request.operation {
        BoardOperation::WorkstreamCreate { workstream_id, .. }
        | BoardOperation::WorkstreamRename { workstream_id, .. }
        | BoardOperation::WorkstreamClose { workstream_id, .. } => workstream_id.clone().into(),
        BoardOperation::TaskCreate { task_id, .. }
        | BoardOperation::TaskUpdate { task_id, .. }
        | BoardOperation::TaskMoveWorkstream { task_id, .. }
        | BoardOperation::TaskSetStatus { task_id, .. }
        | BoardOperation::TaskAction { task_id, .. } => task_id.clone().into(),
        BoardOperation::TaskLink { link } => link.from.clone().into(),
        BoardOperation::TaskUnlink { link } => link.from.clone().into(),
        BoardOperation::SubtaskAdd { task_id, .. } => task_id.clone().into(),
        BoardOperation::SubtaskCheck { subtask_id, .. }
        | BoardOperation::SubtaskMove { subtask_id, .. }
        | BoardOperation::SubtaskRemove { subtask_id } => read
            .subtask_task_metadata(subtask_id)
            .await?
            .map_or_else(|| subtask_id.clone().into(), Into::into),
        BoardOperation::RoleGrant { subject, .. } | BoardOperation::RoleRelease { subject, .. } => {
            match subject {
                RoleSubject::Project(id) => id.clone().into(),
                RoleSubject::Workstream(id) => id.clone().into(),
            }
        }
        BoardOperation::ThreadCreate { post_id, .. } => ThreadId::from(post_id.clone()).into(),
        BoardOperation::ThreadRefile { thread_id, .. }
        | BoardOperation::ThreadPost { thread_id, .. }
        | BoardOperation::ThreadResolve { thread_id, .. }
        | BoardOperation::ThreadUnresolve { thread_id } => thread_id.clone().into(),
        BoardOperation::ReadMarkAdvance {
            scope: ReadScope::Thread { thread_id },
            ..
        } => thread_id.clone().into(),
        BoardOperation::WorkTrailAdd {
            row_id, subject, ..
        } => {
            let resolved = read
                .trail_subject_metadata(row_id)
                .await?
                .or_else(|| subject.clone());
            match resolved {
                Some(TrailSubject::Task(id)) => id.into(),
                Some(TrailSubject::Workstream(id)) => id.into(),
                _ => request.project_id.clone().into(),
            }
        }
        BoardOperation::WorkTrailRedact { row_id, .. } => row_id.clone().into(),
        _ => request.project_id.clone().into(),
    })
}
