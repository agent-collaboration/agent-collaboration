//! Test-only native arrangement for conversation scenarios; WS grant policy has separate proof.
use crate::ProjectStoreRegistry;
use message_board::*;

pub(crate) async fn create(
    registry: &ProjectStoreRegistry,
    project: &ProjectId,
) -> Result<ThreadSubject, Box<dyn std::error::Error + Send + Sync>> {
    let store = registry
        .existing_project(project)
        .await?
        .ok_or("fixture project")?;
    let mut write = store.begin_admission().await?;
    let Some(Record::Project(state)) = write.record(&project.clone().into()).await? else {
        return Err("fixture state".into());
    };
    let stream = WorkstreamId::generate();
    write
        .append_record(Record::Workstream(Workstream {
            envelope: RecordEnvelope {
                id: stream.clone().into(),
                request: None,
                ..state.envelope
            },
            workstream_id: stream.clone(),
            project_id: project.clone(),
            name: "Fixture discussions".to_owned().try_into()?,
            goal: "Conversation fixture subject".into(),
            state: WorkstreamState::Open {},
            revision: SubjectRevision::INITIAL,
        }))
        .await?;
    write.commit().await?;
    Ok(ThreadSubject::Workstream(stream))
}
